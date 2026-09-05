//! Organization inventory stays separate from explicit deep-monitoring targets.
use crate::{
    auth::Auth,
    common::{self, Endpoint},
};
use monitor_core::{
    config::{resolve::Job, types::DiscoveryRoot},
    model::*,
};
use monitor_integrations::{
    projection::{identity, observation, operation},
    transport::{Error, Http},
};
use serde_json::json;
use tokio_util::sync::CancellationToken;
pub async fn collect(
    http: &Http,
    auth: &Auth,
    job: &Job,
    roots: &[DiscoveryRoot],
    cancel: &CancellationToken,
) -> CheckResult {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    result.operations.clear();
    for root in roots.iter().filter(|r| r.provider == job.target.provider) {
        if root.provider == Provider::Aws && root.scope.starts_with("o-") {
            let mut organization = Endpoint::get(
                "organization-scope",
                "https://organizations.us-east-1.amazonaws.com/",
                "/Organization",
            );
            organization.aws = Some((
                "organizations".into(),
                "us-east-1".into(),
                "AWSOrganizationsV20161128.DescribeOrganization".into(),
            ));
            organization.body = Some(json!({}));
            let matched = common::request(http, auth, &organization, job, cancel)
                .await
                .and_then(|value| {
                    if monitor_integrations::projection::text(&value, &["/Organization/Id"])
                        == Some(root.scope.as_str())
                    {
                        Ok(1)
                    } else {
                        Err(Error::Forbidden)
                    }
                });
            result.operations.push(operation(
                &format!("organization-scope/{}", root.scope),
                matched.as_ref().copied(),
                1,
                true,
            ));
            if matched.is_err() {
                continue;
            }
        }
        let mut endpoint = match root.provider {
            Provider::Gcp => Endpoint::get(
                "organization-projects",
                format!(
                    "https://cloudasset.googleapis.com/v1/{}:searchAllResources?assetTypes=cloudresourcemanager.googleapis.com%2FProject&pageSize={}",
                    root.scope, job.settings.page_size
                ),
                "/results",
            ),
            Provider::Azure => Endpoint::get(
                "subscriptions",
                "https://management.azure.com/subscriptions?api-version=2022-12-01",
                "/value",
            ),
            Provider::Aws => {
                let mut e = Endpoint::get(
                    "accounts",
                    "https://organizations.us-east-1.amazonaws.com/",
                    "/Accounts",
                );
                e.body = Some(json!({"MaxResults":20}));
                e.aws = Some((
                    "organizations".into(),
                    "us-east-1".into(),
                    "AWSOrganizationsV20161128.ListAccounts".into(),
                ));
                e
            }
            _ => continue,
        };
        endpoint.id = format!("{}/{}", endpoint.id, root.scope);
        let mut count = 0;
        let mut outcome = Ok(0);
        let mut seen = std::collections::BTreeSet::new();
        let mut pages = 0;
        for _ in 0..job.settings.max_pages {
            pages += 1;
            match common::request(http, auth, &endpoint, job, cancel).await {
                Ok(payload) => {
                    let names = if root.provider == Provider::Gcp {
                        match serde_json::from_value::<
                            google_cloud_asset_v1::model::SearchAllResourcesResponse,
                        >(payload.clone())
                        {
                            Ok(response) => response
                                .results
                                .into_iter()
                                .map(|r| r.name)
                                .collect::<Vec<_>>(),
                            Err(_) => {
                                outcome = Err(Error::Malformed);
                                break;
                            }
                        }
                    } else {
                        if !payload
                            .pointer(&endpoint.items)
                            .is_some_and(|value| value.is_array())
                        {
                            outcome = Err(Error::Malformed);
                            break;
                        }
                        crate::resource_projection::rows(&payload, &endpoint.items)
                            .iter()
                            .filter_map(|r| {
                                monitor_integrations::projection::text(
                                    r,
                                    &["/subscriptionId", "/Id"],
                                )
                            })
                            .map(String::from)
                            .collect()
                    };
                    for name in names {
                        if result.observations.len() >= job.settings.max_assets {
                            outcome = Err(Error::Limit);
                            break;
                        }
                        result.observations.push(observation(
                            job,
                            &endpoint.id,
                            &identity(&name),
                            Data::Inventory {
                                family: "inventory-only-scope".into(),
                                supported: false,
                            },
                        ));
                        count += 1;
                    }
                    let token = monitor_integrations::projection::text(
                        &payload,
                        &["/nextPageToken", "/NextToken", "/nextLink"],
                    )
                    .unwrap_or("");
                    if outcome.is_err() || token.is_empty() {
                        break;
                    }
                    if !seen.insert(token.to_string()) || pages == job.settings.max_pages {
                        outcome = Err(Error::Limit);
                        break;
                    }
                    if root.provider == Provider::Gcp {
                        let Ok(mut url) = url::Url::parse(&endpoint.url) else {
                            outcome = Err(Error::Malformed);
                            break;
                        };
                        let query: Vec<_> = url
                            .query_pairs()
                            .filter(|(key, _)| key != "pageToken")
                            .map(|(k, v)| (k.into_owned(), v.into_owned()))
                            .collect();
                        url.query_pairs_mut()
                            .clear()
                            .extend_pairs(query)
                            .append_pair("pageToken", token);
                        endpoint.url = url.into();
                    } else if let Some(body) = &mut endpoint.body {
                        body["NextToken"] = json!(token);
                    } else if token.starts_with("https://management.azure.com/subscriptions?") {
                        endpoint.url = token.into();
                    } else {
                        outcome = Err(Error::Forbidden);
                        break;
                    }
                }
                Err(error) => {
                    outcome = Err(error);
                    break;
                }
            }
        }
        result.operations.push(operation(
            &endpoint.id,
            outcome.map(|_| count).as_ref().copied(),
            pages,
            true,
        ));
        if root.provider == Provider::Aws {
            let region = job
                .target
                .regions
                .first()
                .map(String::as_str)
                .unwrap_or("us-east-1");
            let mut endpoint = Endpoint::get(
                format!("regions/{}", root.scope),
                format!("https://ec2.{region}.amazonaws.com/"),
                "/regionInfo/item",
            );
            endpoint.aws = Some(("ec2".into(), region.into(), "query:DescribeRegions".into()));
            endpoint.body = Some(
                json!({"Action":"DescribeRegions","Version":"2016-11-15","AllRegions":"true"}),
            );
            let collected = common::collect(http, auth, job, vec![endpoint], cancel).await;
            result.operations.extend(collected.operations);
            result.observations.extend(collected.observations);
        }
    }
    if result.operations.is_empty() {
        result.operations.push(operation(
            "discovery-roots",
            Err(&Error::Unavailable),
            0,
            true,
        ));
    }
    result.finished_at = chrono::Utc::now();
    result
}

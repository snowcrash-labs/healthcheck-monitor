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

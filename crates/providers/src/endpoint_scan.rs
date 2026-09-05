//! Bounded per-endpoint collection and pagination.
use crate::common::{Endpoint, Source};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{
    projection::{operation, text},
    transport::Error,
};
use serde_json::Value;
use std::collections::VecDeque;
use tokio_util::sync::CancellationToken;
#[derive(Clone)]
pub struct Fetched {
    pub result: CheckResult,
    pub followups: Vec<Endpoint>,
}
pub async fn fetch<S: Source>(
    source: &S,
    job: &Job,
    endpoint: Endpoint,
    cancel: &CancellationToken,
) -> Fetched {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    result.operations.clear();
    let mut pending = VecDeque::new();
    let mut endpoint = endpoint;
    let mut outcome = Ok(0usize);
    let mut pages = 0;
    let mut previous_token = String::new();
    for page in 0..job.settings.max_pages {
        pages = page + 1;
        match source.request(&endpoint, job, cancel).await {
            Ok(payload) => {
                let rows = crate::resource_projection::rows(&payload, &endpoint.items);
                if !rows.is_empty() {
                    for row in &rows {
                        if result.observations.len() >= job.settings.max_assets {
                            outcome = Err(Error::Limit);
                            break;
                        }
                        let projected = crate::resource_projection::project(job, &endpoint, row);
                        let available = job
                            .settings
                            .max_assets
                            .saturating_sub(result.observations.len());
                        if projected.len() > available {
                            outcome = Err(Error::Limit);
                        }
                        result
                            .observations
                            .extend(projected.into_iter().take(available));
                        for detail in crate::details::followups(job, &endpoint, row) {
                            if pending.len() >= job.settings.ready_queue {
                                outcome = Err(Error::Limit);
                                break;
                            }
                            pending.push_back(detail);
                        }
                    }
                    outcome = outcome.map(|n| n + rows.len());
                } else if endpoint.items.is_empty() {
                    result
                        .observations
                        .extend(crate::resource_projection::project(
                            job, &endpoint, &payload,
                        ));
                    outcome = Ok(1);
                } else if payload.as_object().is_some_and(|m| m.is_empty())
                    || payload.pointer(&endpoint.items).is_some_and(|v| {
                        v.as_array().is_some_and(|a| a.is_empty()) || v.as_str() == Some("")
                    })
                {
                    outcome = Ok(0);
                } else {
                    outcome = Err(Error::Malformed);
                }
                if outcome.is_err() {
                    break;
                }
                let token = text(
                    &payload,
                    &[
                        "/nextPageToken",
                        "/nextToken",
                        "/NextToken",
                        "/nextLink",
                        "/NextMarker",
                        "/ContinuationToken",
                        "/$skipToken",
                        "/DescribeDBInstancesResult/Marker",
                        "/DescribeDBClustersResult/Marker",
                        "/DescribeCacheClustersResult/Marker",
                        "/DescribeReplicationGroupsResult/Marker",
                    ],
                )
                .unwrap_or("");
                if token.is_empty() {
                    break;
                }
                if token == previous_token || page + 1 == job.settings.max_pages {
                    outcome = Err(Error::Limit);
                    break;
                }
                previous_token = token.into();
                if token.starts_with("https://") {
                    let next = url::Url::parse(token).map_err(|_| Error::Malformed);
                    let old = url::Url::parse(&endpoint.url).map_err(|_| Error::Malformed);
                    match (next, old) {
                        (Ok(next), Ok(old))
                            if next.origin() == old.origin()
                                && next.path().starts_with(&format!(
                                    "/subscriptions/{}/",
                                    job.target.scope
                                )) =>
                        {
                            endpoint.url = next.into()
                        }
                        _ => {
                            outcome = Err(Error::Forbidden);
                            break;
                        }
                    }
                } else if let Some(body) = &mut endpoint.body {
                    if payload.get("$skipToken").is_some() {
                        body["options"]["$skipToken"] = Value::String(token.into());
                        continue;
                    }
                    let field = if endpoint.aws.is_some() {
                        if payload.get("NextToken").is_some() {
                            "NextToken"
                        } else if endpoint
                            .aws
                            .as_ref()
                            .is_some_and(|(_, _, target)| target.starts_with("query:"))
                        {
                            "Marker"
                        } else {
                            "nextToken"
                        }
                    } else {
                        "pageToken"
                    };
                    body[field] = Value::String(token.into());
                } else {
                    let Ok(mut url) = url::Url::parse(&endpoint.url) else {
                        outcome = Err(Error::Malformed);
                        break;
                    };
                    let field = if payload.get("ContinuationToken").is_some() {
                        "continuation-token"
                    } else if payload.get("NextMarker").is_some() {
                        "Marker"
                    } else if job.target.provider == Provider::Gcp {
                        "pageToken"
                    } else {
                        "NextToken"
                    };
                    let pairs: Vec<_> = url
                        .query_pairs()
                        .filter(|(k, _)| k != field)
                        .map(|(k, v)| (k.into_owned(), v.into_owned()))
                        .collect();
                    url.query_pairs_mut()
                        .clear()
                        .extend_pairs(pairs)
                        .append_pair(field, token);
                    endpoint.url = url.into();
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
        outcome.as_ref().copied(),
        pages,
        job.settings.required,
    ));
    result.finished_at = chrono::Utc::now();
    Fetched {
        result,
        followups: pending.into_iter().collect(),
    }
}

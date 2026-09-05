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
    if endpoint.id.starts_with("batch-") && endpoint.aws.is_some() {
        return crate::aws_batches::fetch(source, job, endpoint, cancel).await;
    }
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    result.operations.clear();
    let mut pending = VecDeque::new();
    let mut queued_bytes = 0usize;
    let queue_limit = job.settings.memory_bytes / 4 / job.settings.concurrency.max(1);
    let mut budget = monitor_core::collection_budget::Limit::new(&job.settings);
    let mut endpoint = endpoint;
    let mut outcome = Ok(0usize);
    let mut pages = 0;
    let mut previous_token = String::new();
    for page in 0..job.settings.max_pages {
        pages = page + 1;
        match source.request(&endpoint, job, cancel).await {
            Ok(payload) => {
                let rows = crate::resource_projection::rows(&payload, &endpoint.items);
                let batches = crate::aws_batches::followups(&endpoint, &rows);
                if let Some(batches) = &batches {
                    for batch in batches {
                        let size = batch.bytes();
                        if pending.len() >= job.settings.ready_queue
                            || size > queue_limit.saturating_sub(queued_bytes)
                        {
                            outcome = Err(Error::Limit);
                            break;
                        }
                        queued_bytes += size;
                        pending.push_back(batch.clone());
                    }
                }
                if !rows.is_empty() {
                    for row in &rows {
                        if result.observations.len() >= job.settings.max_assets {
                            outcome = Err(Error::Limit);
                            break;
                        }
                        let projected = crate::resource_projection::project(job, &endpoint, row);
                        if let Err(error) = crate::metadata_fields::validate(job, &endpoint, row) {
                            outcome = Err(error);
                        }
                        if !budget.observations(&mut result.observations, projected) {
                            outcome = Err(Error::Limit);
                        }
                        for detail in if batches.is_none() {
                            crate::details::followups(job, &endpoint, row)
                        } else {
                            vec![]
                        } {
                            let size = detail.bytes();
                            if pending.len() >= job.settings.ready_queue
                                || size > queue_limit.saturating_sub(queued_bytes)
                            {
                                outcome = Err(Error::Limit);
                                break;
                            }
                            queued_bytes += size;
                            pending.push_back(detail);
                        }
                        if outcome.is_err() {
                            break;
                        }
                    }
                    outcome = outcome.map(|n| n + rows.len());
                } else if endpoint.items.is_empty() {
                    let projected = crate::resource_projection::project(job, &endpoint, &payload);
                    outcome = crate::metadata_fields::validate(job, &endpoint, &payload).map(|_| 1);
                    if !budget.observations(&mut result.observations, projected) {
                        outcome = Err(Error::Limit);
                    }
                } else if payload.as_object().is_some_and(|m| m.is_empty())
                    || endpoint.items == "/items"
                        && payload
                            .get("items")
                            .and_then(Value::as_object)
                            .is_some_and(|scopes| {
                                scopes.values().all(|scope| {
                                    text(scope, &["/warning/code"]) == Some("NO_RESULTS_ON_PAGE")
                                })
                            })
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
                if endpoint.id.starts_with("registry-manifest/")
                    && let Err(error) =
                        crate::registry_manifests::validate(job, &endpoint, &payload)
                {
                    outcome = Err(error);
                    break;
                }
                match crate::pagination::advance(
                    &mut endpoint,
                    job,
                    &payload,
                    &previous_token,
                    pages,
                ) {
                    Ok(Some(token)) => previous_token = token,
                    Ok(None) => break,
                    Err(error) => {
                        outcome = Err(error);
                        break;
                    }
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

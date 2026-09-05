//! Bounded dynamic inventory fan-out; continuation pages stay inside their owning branch.
use crate::common::{Endpoint, Source, cache_key};
use futures::{StreamExt, stream::FuturesUnordered};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{projection::operation, transport::Error};
use std::collections::{BTreeSet, VecDeque};
use tokio_util::sync::CancellationToken;

pub async fn collect<S: Source>(
    source: &S,
    job: &Job,
    endpoints: Vec<Endpoint>,
    cancel: &CancellationToken,
) -> CheckResult {
    let mut result = crate::router::base(job);
    let mut initial = endpoints.into_iter();
    let mut pending = VecDeque::new();
    let mut queued_bytes = 0usize;
    let queue_limit = job.settings.memory_bytes / 4 / job.settings.concurrency.max(1);
    let mut budget = monitor_core::collection_budget::Limit::new(&job.settings);
    let mut visited = BTreeSet::new();
    let mut running = FuturesUnordered::new();
    let width = monitor_integrations::admission::width(&job.settings);
    loop {
        while running.len() < width && !budget.exhausted() {
            let endpoint = if let Some(endpoint) = pending.pop_front() {
                queued_bytes = queued_bytes.saturating_sub(Endpoint::bytes(&endpoint));
                endpoint
            } else if let Some(endpoint) = initial.next() {
                endpoint
            } else {
                break;
            };
            let key = cache_key(job, &endpoint);
            if !visited.insert(key.clone()) {
                continue;
            }
            if visited.len() > job.settings.max_assets {
                budget.mark_limited();
                break;
            }
            running.push(async move {
                if cancel.is_cancelled() {
                    return crate::endpoint_scan::Fetched {
                        result: CheckResult::failure(
                            job.target.name.clone(),
                            job.check,
                            job.revision.clone(),
                            Coverage::Cancelled,
                        ),
                        followups: vec![],
                    };
                }
                if let Some(cache) = source.cache() {
                    cache.load(source, job, endpoint, cancel, key).await
                } else {
                    crate::endpoint_scan::fetch(source, job, endpoint, cancel).await
                }
            });
        }
        let Some(mut fetched) = running.next().await else {
            if !pending.is_empty() || initial.len() > 0 {
                budget.mark_limited();
            }
            break;
        };
        for followup in fetched.followups {
            let bytes = followup.bytes();
            if pending.len() >= job.settings.ready_queue
                || bytes > queue_limit.saturating_sub(queued_bytes)
            {
                for op in &mut fetched.result.operations {
                    op.coverage = Coverage::Truncated;
                }
                break;
            }
            queued_bytes += bytes;
            pending.push_back(followup);
        }
        for op in &mut fetched.result.operations {
            op.required = job.settings.required;
        }
        budget.merge(&mut result, fetched.result);
    }
    budget.finish(&mut result, job.settings.required);
    if result.operations.is_empty() {
        result.operations.push(operation(
            "not-configured",
            Err(&Error::Missing),
            0,
            job.settings.required,
        ));
    }
    result.operations.sort_by(|a, b| a.id.cmp(&b.id));
    result
        .observations
        .sort_by(|a, b| a.resource.cmp(&b.resource));
    result.finished_at = chrono::Utc::now();
    result
}

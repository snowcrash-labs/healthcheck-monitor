//! Native CloudWatch metric batches.
use crate::{auth::Auth, metrics::empty};
use chrono::Utc;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{projection::operation, transport::Error};
use tokio_util::sync::CancellationToken;
pub async fn aws(auth: &Auth, job: &Job, cancel: &CancellationToken) -> CheckResult {
    with_namespaces(auth, job, cancel, None).await
}
pub async fn with_namespaces(
    auth: &Auth,
    job: &Job,
    cancel: &CancellationToken,
    expected: Option<&crate::aws_metric_inventory::Namespaces>,
) -> CheckResult {
    let mut result = empty(job);
    let Auth::Aws(clients) = auth else {
        return result;
    };
    result.operations.clear();
    for region in &crate::aws_metric_plan::regions(job) {
        if expected.is_some_and(|expected| !expected.contains_key(region)) {
            continue;
        }
        let client = match clients.cloudwatch(region, &job.settings).await {
            Ok(client) => client,
            Err(error) => {
                result
                    .operations
                    .push(operation("cloudwatch-client", Err(&error), 0, true));
                continue;
            }
        };
        let queries = if job.target.metrics.is_empty() {
            let (queries, operations) = crate::aws_metric_discovery::discover(
                &client,
                job,
                region,
                expected.and_then(|expected| expected.get(region)),
            )
            .await;
            result.operations.extend(operations);
            queries
        } else {
            crate::aws_metric_plan::configured(job, region)
        };
        for batch in queries.chunks(500) {
            let remaining = job
                .settings
                .max_series
                .saturating_sub(result.observations.len());
            let take = remaining.min(batch.len());
            if take > 0 {
                let collected =
                    crate::aws_metric_batch::collect(&client, job, region, &batch[..take], cancel)
                        .await;
                result.operations.extend(collected.operations);
                result.observations.extend(collected.observations);
            }
            for query in &batch[take..] {
                result.operations.push(operation(
                    &format!("{region}/{}", query.name),
                    Err(&Error::Limit),
                    0,
                    true,
                ));
            }
        }
    }
    if result.operations.is_empty() && expected.is_none() {
        result.operations.push(operation(
            "cloudwatch-metrics",
            Err(&Error::Missing),
            0,
            true,
        ));
    }
    result.finished_at = Utc::now();
    result
}

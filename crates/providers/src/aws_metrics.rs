//! Native CloudWatch metric batches.
use crate::{auth::Auth, metrics::empty};
use chrono::Utc;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{projection::operation, transport::Error};
use tokio_util::sync::CancellationToken;
pub async fn aws(auth: &Auth, job: &Job, cancel: &CancellationToken) -> CheckResult {
    let mut result = empty(job);
    let Auth::Aws(clients) = auth else {
        return result;
    };
    result.operations.clear();
    for region in &job.target.regions {
        let client = match clients.cloudwatch(region).await {
            Ok(client) => client,
            Err(error) => {
                result
                    .operations
                    .push(operation("cloudwatch-client", Err(&error), 0, true));
                continue;
            }
        };
        let queries = if job.target.metrics.is_empty() {
            let (queries, operations) = crate::aws_metric_discovery::discover(&client, job).await;
            result.operations.extend(operations);
            queries
        } else {
            job.target.metrics.clone()
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
    if result.operations.is_empty() {
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

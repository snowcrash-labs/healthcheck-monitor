//! Operational metric windows, with explicit missing-series coverage.
use crate::{auth::Auth, common};
use chrono::Utc;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{projection::operation, transport::Http};
use tokio_util::sync::CancellationToken;

pub(crate) fn empty(job: &Job) -> CheckResult {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    if !job.target.metrics.is_empty() {
        result.operations.clear();
    }
    result
}
pub async fn gcp(http: &Http, auth: &Auth, job: &Job, cancel: &CancellationToken) -> CheckResult {
    gcp_from(
        &common::NativeSource {
            http,
            auth,
            cache: None,
            dedupe: None,
        },
        job,
        cancel,
    )
    .await
}
pub async fn gcp_from<S: common::Source>(
    source: &S,
    job: &Job,
    cancel: &CancellationToken,
) -> CheckResult {
    let mut result = empty(job);
    for (index, query) in job.target.metrics.iter().enumerate() {
        let remaining = job
            .settings
            .max_series
            .saturating_sub(result.observations.len());
        let limit = remaining / (job.target.metrics.len() - index);
        let (observations, outcome, pages) =
            crate::gcp_metric_pages::collect(source, job, query, limit, cancel).await;
        result.observations.extend(observations);
        result.operations.push(operation(
            &query.name,
            outcome.as_ref().copied(),
            pages,
            job.settings.required,
        ));
    }
    result.finished_at = Utc::now();
    result
}
pub use crate::aws_metrics::aws;

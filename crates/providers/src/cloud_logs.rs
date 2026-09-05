//! Native entry points for cloud diagnostic windows.
use crate::{auth::Auth, common::NativeSource};
use monitor_core::{config::resolve::Job, model::CheckResult};
use monitor_integrations::transport::Http;
use tokio_util::sync::CancellationToken;
pub async fn aws(http: &Http, auth: &Auth, job: &Job, cancel: &CancellationToken) -> CheckResult {
    crate::aws_logs::collect_from(
        &NativeSource {
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
pub async fn azure(http: &Http, auth: &Auth, job: &Job, cancel: &CancellationToken) -> CheckResult {
    crate::azure_logs::collect_from(
        &NativeSource {
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

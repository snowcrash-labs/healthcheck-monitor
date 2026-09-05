//! Bounded ARM activity metadata excludes caller claims, request bodies and descriptions.
use crate::common::{Endpoint, Source};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::projection::{identity, observation, text, timestamp};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
pub async fn collect<S: Source>(source: &S, job: &Job, cancel: &CancellationToken) -> CheckResult {
    let mut bounded = job.clone();
    bounded.settings.max_assets = job.settings.log_entries.min(job.settings.max_assets);
    let now = chrono::Utc::now();
    let mut endpoint = Endpoint::get(
        "activity",
        format!(
            "https://management.azure.com/subscriptions/{}/providers/Microsoft.Insights/eventtypes/management/values",
            job.target.scope
        ),
        "/value",
    );
    let Ok(mut url) = url::Url::parse(&endpoint.url) else {
        return CheckResult::failure(
            job.target.name.clone(),
            job.check,
            job.revision.clone(),
            Coverage::Malformed,
        );
    };
    url.query_pairs_mut()
        .append_pair("api-version", "2015-04-01")
        .append_pair(
            "$select",
            "eventDataId,eventTimestamp,resourceId,operationName,status,level",
        )
        .append_pair(
            "$filter",
            &format!(
                "eventTimestamp ge '{}' and eventTimestamp le '{}'",
                (now - chrono::Duration::seconds(job.settings.log_window.0 as i64)).to_rfc3339(),
                now.to_rfc3339()
            ),
        );
    endpoint.url = url.into();
    crate::common::collect_from(source, &bounded, vec![endpoint], cancel).await
}
pub fn project(job: &Job, endpoint: &Endpoint, value: &Value) -> Vec<Observation> {
    let Some(resource) = text(value, &["/resourceId"])
        .filter(|resource| crate::azure_metric_discovery::valid_resource(job, resource))
    else {
        return vec![];
    };
    let id = text(value, &["/eventDataId"]).unwrap_or("event");
    vec![observation(
        job,
        &endpoint.id,
        &format!("{resource}/{id}"),
        Data::Activity {
            operation: identity(text(value, &["/operationName/value"]).unwrap_or("unknown")),
            state: monitor_integrations::projection::state(text(value, &["/status/value"])),
            event_at: timestamp(value, &["/eventTimestamp"]),
        },
    )]
}

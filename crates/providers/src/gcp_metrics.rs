//! Automatic metric registration follows successful inventories, including shared cached reads.
use crate::common::{Source, collect_from};
use monitor_core::{config::resolve::Job, model::*};
use tokio_util::sync::CancellationToken;
pub async fn collect<S: Source>(source: &S, job: &Job, cancel: &CancellationToken) -> CheckResult {
    if !job.target.metrics.is_empty() {
        return crate::metrics::gcp_from(source, job, cancel).await;
    }
    let mut inventory_job = job.clone();
    inventory_job.check = Check::Inventory;
    let endpoints = crate::gcp::endpoints(&inventory_job)
        .into_iter()
        .filter(|endpoint| {
            matches!(
                endpoint.id.split('/').next(),
                Some(
                    "clusters"
                        | "sql"
                        | "redis"
                        | "valkey"
                        | "pubsub-subscriptions"
                        | "cloud-run"
                        | "buckets"
                        | "instances"
                )
            )
        })
        .collect();
    let mut result = collect_from(source, job, endpoints, cancel).await;
    result.check = job.check;
    let mut metrics = job.clone();
    metrics.target.metrics = crate::metric_catalog::gcp()
        .into_iter()
        .filter(|query| {
            let family = match query.namespace.as_str() {
                "k8s_node" | "k8s_container" => "clusters",
                "cloudsql_database" => "sql",
                "redis_instance" => "redis",
                "memorystore.googleapis.com/Instance" => "valkey",
                "pubsub_subscription" => "pubsub-subscriptions",
                "cloud_run_revision" => "cloud-run",
                "gcs_bucket" => "buckets",
                "gce_instance" => "instances",
                _ => return false,
            };
            if job.check == Check::Queues && family != "pubsub-subscriptions" {
                return false;
            }
            result.operations.iter().any(|operation| {
                operation.id.starts_with(&format!("{family}/"))
                    && (operation.records > 0 || operation.coverage != Coverage::Complete)
            })
        })
        .collect();
    if !metrics.target.metrics.is_empty() {
        let collected = crate::metrics::gcp_from(source, &metrics, cancel).await;
        result.operations.extend(collected.operations);
        result.observations.extend(collected.observations);
    }
    result.finished_at = chrono::Utc::now();
    result
}

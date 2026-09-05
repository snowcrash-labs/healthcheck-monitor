//! Operational metric registration follows inventory and preserves bounded page evidence.
use chrono::{Duration, Utc};
use monitor_core::{
    config::{
        resolve::{Job, Selection},
        types::Config,
    },
    model::*,
};
use monitor_integrations::transport::Error;
use monitor_providers::{
    common::{Endpoint, Source},
    gcp_metrics, metrics,
};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex};
use tokio_util::sync::CancellationToken;
struct Empty;
impl Source for Empty {
    async fn request(
        &self,
        endpoint: &Endpoint,
        _: &Job,
        _: &CancellationToken,
    ) -> Result<Value, Error> {
        assert!(!endpoint.url.contains("/timeSeries"));
        let mut value = json!({});
        value[endpoint.items.trim_start_matches('/')] = json!([]);
        Ok(value)
    }
}
struct Pages(Mutex<VecDeque<Value>>);
impl Source for Pages {
    async fn request(&self, _: &Endpoint, _: &Job, _: &CancellationToken) -> Result<Value, Error> {
        self.0
            .lock()
            .map_err(|_| Error::Unavailable)?
            .pop_front()
            .ok_or(Error::Missing)
    }
}
fn job() -> Result<Job, Box<dyn std::error::Error>> {
    Config::parse("version=1\n[[targets]]\nname='test'\nprovider='gcp'\nscope='project'\nregions=['us-central1']")?.resolve(&Selection::default())?.jobs.into_iter().find(|job|job.check==Check::Metrics).ok_or_else(||"missing job".into())
}
#[tokio::test]
async fn absent_services_do_not_create_required_missing_metric_queries()
-> Result<(), Box<dyn std::error::Error>> {
    let result = gcp_metrics::collect(&Empty, &job()?, &CancellationToken::new()).await;
    assert!(result.complete());
    assert!(result.observations.is_empty());
    Ok(())
}
#[tokio::test]
async fn split_metric_pages_form_one_window_with_readable_resource_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let mut job = job()?;
    job.target.metrics = vec![serde_json::from_value(
        json!({"name":"cpu","namespace":"gce_instance","metric":"compute.googleapis.com/instance/cpu/utilization","resource":"vm","capacity":1.0}),
    )?];
    let now = Utc::now();
    let row = |range: std::ops::Range<i64>| json!({"resource":{"type":"gce_instance","labels":{"instance_id":"instance-123","project_id":"project","customer_id":"forbidden-customer"}},"metric":{"type":"compute.googleapis.com/instance/cpu/utilization"},"points":range.map(|minute|json!({"interval":{"endTime":(now-Duration::minutes(minute)).to_rfc3339()},"value":{"doubleValue":0.95}})).collect::<Vec<_>>()});
    let pages = Pages(Mutex::new(VecDeque::from([
        json!({"timeSeries":[row(0..5)],"nextPageToken":"second"}),
        json!({"timeSeries":[row(5..11)]}),
    ])));
    let result = metrics::gcp_from(&pages, &job, &CancellationToken::new()).await;
    assert!(result.complete());
    assert_eq!(result.operations[0].pages, 2);
    assert_eq!(result.observations.len(), 1);
    assert!(matches!(
        result.observations[0].data,
        Data::Metric {
            window_seconds: 600,
            ..
        }
    ));
    assert!(result.observations[0].resource.contains("instance-123"));
    assert!(!serde_json::to_string(&result)?.contains("forbidden-customer"));
    Ok(())
}
#[test]
fn valkey_and_redis_clusters_use_separate_apis_and_global_metadata_ids_are_stable()
-> Result<(), Box<dyn std::error::Error>> {
    let mut job = job()?;
    job.check = Check::Inventory;
    let endpoints = monitor_providers::gcp::endpoints(&job);
    assert!(endpoints.iter().any(|endpoint| {
        endpoint.id.starts_with("valkey/")
            && endpoint
                .url
                .starts_with("https://memorystore.googleapis.com/")
    }));
    assert!(
        endpoints
            .iter()
            .any(|endpoint| endpoint.id.starts_with("redis-clusters/")
                && endpoint.url.starts_with("https://redis.googleapis.com/"))
    );
    assert!(
        endpoints
            .iter()
            .any(|endpoint| endpoint.id == "kms-keyrings/global")
    );
    job.target.regions = vec!["europe-west1".into()];
    let changed = monitor_providers::gcp::endpoints(&job);
    assert!(
        endpoints
            .iter()
            .filter(|endpoint| endpoint.id.starts_with("sql/"))
            .all(|endpoint| changed.iter().any(|next| next.id == endpoint.id))
    );
    Ok(())
}
#[test]
fn queue_gauges_keep_the_latest_value_while_capacity_uses_a_sustained_minimum()
-> Result<(), Box<dyn std::error::Error>> {
    use monitor_core::config::types::Aggregation;
    let queries = monitor_providers::metric_catalog::gcp();
    let queue = queries
        .iter()
        .find(|query| query.name == "pubsub-backlog")
        .ok_or("missing backlog")?;
    let capacity = queries
        .iter()
        .find(|query| query.name == "redis-cluster-memory")
        .ok_or("missing cluster memory")?;
    assert!(matches!(queue.aggregation, Aggregation::Latest));
    assert!(matches!(capacity.aggregation, Aggregation::Minimum));
    let now = Utc::now();
    let observation = monitor_providers::metric_window::project(
        &job()?,
        queue,
        "subscription",
        vec![(now - Duration::minutes(10), 0.0), (now, 12.0)],
    )?;
    assert!(matches!(
        observation.data,
        Data::Metric {
            value: 12.0,
            window_seconds: 0,
            ..
        }
    ));
    Ok(())
}
#[tokio::test]
async fn latency_distributions_require_samples_and_failure_series_have_explicit_thresholds()
-> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now();
    for (name, value, code, complete) in [
        (
            "run-latency-ms",
            json!({"distributionValue":{"count":"2","mean":125.0}}),
            "200",
            true,
        ),
        (
            "run-latency-ms",
            json!({"distributionValue":{"count":"0","mean":0.0}}),
            "200",
            false,
        ),
        ("run-requests", json!({"int64Value":"3"}), "503", true),
    ] {
        let mut job = job()?;
        job.target.metrics = monitor_providers::metric_catalog::gcp()
            .into_iter()
            .filter(|query| query.name == name)
            .collect();
        let query = job.target.metrics.first().ok_or("missing preset")?;
        let pages = Pages(Mutex::new(VecDeque::from([
            json!({"timeSeries":[{"resource":{"type":query.namespace,"labels":{"service_name":"api"}},"metric":{"type":query.metric,"labels":{"response_code":code}},"points":[{"interval":{"endTime":now.to_rfc3339()},"value":value}]}]}),
        ])));
        let result = metrics::gcp_from(&pages, &job, &CancellationToken::new()).await;
        assert_eq!(result.complete(), complete);
        if name == "run-requests" {
            assert!(matches!(
                result.observations[0].data,
                Data::Metric {
                    value: 3.0,
                    warning: Some(1.0),
                    ..
                }
            ));
        } else if complete {
            assert!(matches!(
                result.observations[0].data,
                Data::Metric { value: 125.0, .. }
            ));
        } else {
            assert!(result.observations.is_empty());
        }
    }
    Ok(())
}

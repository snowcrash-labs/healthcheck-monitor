//! Azure metric contracts exercise discovery, batching, aggregation, and partial failures.
use monitor_core::{
    config::{
        resolve::{Job, Selection},
        types::Config,
    },
    model::*,
};
use monitor_integrations::transport::Error;
use monitor_providers::{
    azure_metrics::collect_from,
    common::{Endpoint, Source},
};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex};
use tokio_util::sync::CancellationToken;

struct Fake {
    responses: Mutex<VecDeque<Result<Value, Error>>>,
    requests: Mutex<Vec<String>>,
}
impl Source for Fake {
    async fn request(
        &self,
        endpoint: &Endpoint,
        _: &Job,
        _: &CancellationToken,
    ) -> Result<Value, Error> {
        self.requests
            .lock()
            .map_err(|_| Error::Unavailable)?
            .push(endpoint.url.clone());
        self.responses
            .lock()
            .map_err(|_| Error::Unavailable)?
            .pop_front()
            .ok_or(Error::Missing)?
    }
}
fn job() -> Result<Job, Box<dyn std::error::Error>> {
    Config::parse(
        "version=1\n[[targets]]\nname='azure'\nprovider='azure'\nscope='sub'\nregions=['westus']",
    )?
    .resolve(&Selection::default())?
    .jobs
    .into_iter()
    .find(|job| job.check == Check::Metrics)
    .ok_or_else(|| "missing metric job".into())
}
fn fake(values: Vec<Result<Value, Error>>) -> Fake {
    Fake {
        responses: Mutex::new(values.into()),
        requests: Mutex::new(Vec::new()),
    }
}
fn graph() -> Value {
    json!({"data":[{"id":"/subscriptions/sub/resourceGroups/test/providers/Microsoft.Cache/redis/cache","name":"cache","type":"Microsoft.Cache/redis"}]})
}
fn definitions() -> Value {
    json!({"value":[{"name":{"value":"usedmemorypercentage"},"unit":"Percent","primaryAggregationType":"Average","supportedAggregationTypes":["Minimum","Average"]},{"name":{"value":"serverLoad"},"unit":"Percent","primaryAggregationType":"Average","supportedAggregationTypes":["Minimum","Average"]}]})
}
fn metric(name: &str, aggregation: &str, value: f64) -> Value {
    json!({"name":{"value":name},"timeseries":[{"metadatavalues":[{"name":{"value":"ShardId"},"value":"0"}],"data":[{"timeStamp":"2026-09-05T00:00:00Z",aggregation:value},{"timeStamp":"2026-09-05T00:01:00Z",aggregation:value+1.0}]}]})
}
#[tokio::test]
async fn availability_percentages_are_not_capacity_pressure()
-> Result<(), Box<dyn std::error::Error>> {
    let source = fake(vec![
        Ok(graph()),
        Ok(
            json!({"value":[{"name":{"value":"Availability"},"unit":"Percent","primaryAggregationType":"Average","supportedAggregationTypes":["Minimum","Average"]}]}),
        ),
        Ok(json!({"value":[metric("Availability","average",99.0)]})),
    ]);
    let result = collect_from(&source, &job()?, &CancellationToken::new()).await;
    assert!(result.complete());
    assert!(
        result
            .observations
            .iter()
            .any(|obs| matches!(obs.data, Data::Metric { capacity: None, .. }))
    );
    Ok(())
}
#[tokio::test]
async fn discovers_metrics_and_applies_the_documented_aggregation()
-> Result<(), Box<dyn std::error::Error>> {
    let source = fake(vec![
        Ok(graph()),
        Ok(definitions()),
        Ok(
            json!({"value":[metric("usedmemorypercentage", "minimum", 91.0),metric("serverLoad", "minimum", 50.0)]}),
        ),
    ]);
    let result = collect_from(&source, &job()?, &CancellationToken::new()).await;
    assert!(result.complete());
    assert!(result.observations.iter().any(|obs| matches!(
        obs.data,
        Data::Metric {
            value: 91.0,
            capacity: Some(100.0),
            window_seconds: 60,
            ..
        }
    )));
    let requests = source.requests.lock().map_err(|_| "lock")?;
    assert_eq!(requests.len(), 3);
    assert!(requests[2].contains("aggregation=Minimum"));
    let url: url::Url = requests[2].parse()?;
    let timespan = url
        .query_pairs()
        .find(|(key, _)| key == "timespan")
        .ok_or("timespan")?
        .1
        .into_owned();
    assert!(timespan.split('/').all(|instant| instant.ends_with('Z')));
    assert!(!timespan.contains('+'));
    Ok(())
}
#[tokio::test]
async fn batches_twenty_metric_names_and_keeps_independent_empty_windows()
-> Result<(), Box<dyn std::error::Error>> {
    let mut job = job()?;
    for i in 0..21 {
        job.target.metrics.push(serde_json::from_value(json!({"name":format!("metric{i}"),"metric":format!("Errors{i}"),"namespace":"Microsoft.Cache/redis","resource":"/subscriptions/sub/resourceGroups/test/providers/Microsoft.Cache/redis/cache","aggregation":"sum"}))?);
    }
    let source = fake(vec![
        Ok(
            json!({"value":(0..19).map(|i|metric(&format!("Errors{i}"),"total",1.0)).collect::<Vec<_>>()}),
        ),
        Err(Error::Denied),
    ]);
    let result = collect_from(&source, &job, &CancellationToken::new()).await;
    assert_eq!(result.observations.len(), 19);
    assert_eq!(
        result
            .operations
            .iter()
            .filter(|op| op.coverage == Coverage::Missing)
            .count(),
        1
    );
    assert_eq!(
        result
            .operations
            .iter()
            .filter(|op| op.coverage == Coverage::Denied)
            .count(),
        1
    );
    assert!(
        result
            .observations
            .iter()
            .all(|obs| matches!(obs.data, Data::Metric { value: 3.0, .. }))
    );
    assert_eq!(source.requests.lock().map_err(|_| "lock")?.len(), 2);
    Ok(())
}
#[tokio::test]
async fn missing_and_excess_series_are_explicit_and_resource_scope_is_checked()
-> Result<(), Box<dyn std::error::Error>> {
    let mut job = job()?;
    job.settings.max_series = 1;
    job.target.metrics.push(serde_json::from_value(json!({"name":"cpu","metric":"Percentage CPU","namespace":"Microsoft.Compute/virtualMachines","resource":"/subscriptions/another/resourceGroups/test/providers/Microsoft.Compute/virtualMachines/test"}))?);
    let source = fake(vec![]);
    let result = collect_from(&source, &job, &CancellationToken::new()).await;
    assert_eq!(result.operations[0].coverage, Coverage::Denied);
    assert!(source.requests.lock().map_err(|_| "lock")?.is_empty());
    job.target.metrics[0].resource =
        "/subscriptions/sub/resourceGroups/test/providers/Microsoft.Compute/virtualMachines/test"
            .into();
    let mut row = metric("Percentage CPU", "minimum", 95.0);
    row["timeseries"]
        .as_array_mut()
        .ok_or("series")?
        .push(json!({"data":[]}));
    let source = fake(vec![Ok(json!({"value":[row]}))]);
    let result = collect_from(&source, &job, &CancellationToken::new()).await;
    assert_eq!(result.observations.len(), 1);
    assert_eq!(result.operations[0].coverage, Coverage::Truncated);
    Ok(())
}

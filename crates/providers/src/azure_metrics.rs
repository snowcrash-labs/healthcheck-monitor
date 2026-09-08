//! Bounded ARM metric batches with stable series identities and explicit empty windows.
use crate::common::{Endpoint, Source};
use chrono::{Duration, Utc};
use monitor_core::{
    config::{
        resolve::Job,
        types::{Aggregation, MetricQuery},
    },
    model::*,
};
use monitor_integrations::{
    projection::{number, operation, text, timestamp},
    transport::Error,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

pub async fn collect_from<S: Source>(
    source: &S,
    job: &Job,
    cancel: &CancellationToken,
) -> CheckResult {
    let (queries, mut result) = if job.target.metrics.is_empty() {
        crate::azure_metric_discovery::discover(source, job, cancel).await
    } else {
        (job.target.metrics.clone(), crate::metrics::empty(job))
    };
    let mut groups: BTreeMap<String, Vec<&MetricQuery>> = BTreeMap::new();
    for query in &queries {
        let key = serde_json::to_string(&(
            &query.resource,
            &query.namespace,
            &query.dimensions,
            aggregation(query.aggregation),
        ))
        .unwrap_or_default();
        groups.entry(key).or_default().push(query);
    }
    let mut series_count = 0;
    for group in groups.values() {
        for batch in group.chunks(20) {
            let endpoint = match endpoint(job, batch) {
                Ok(endpoint) => endpoint,
                Err(error) => {
                    for query in batch {
                        result.operations.push(operation(
                            &query.name,
                            Err(&error),
                            0,
                            job.settings.required,
                        ));
                    }
                    continue;
                }
            };
            match source.request(&endpoint, job, cancel).await {
                Ok(value) => {
                    for query in batch {
                        let (observations, outcome) = project(
                            job,
                            query,
                            &value,
                            job.settings.max_series.saturating_sub(series_count),
                        );
                        series_count += observations.len();
                        result.observations.extend(observations);
                        result.operations.push(operation(
                            &query.name,
                            outcome.as_ref().copied(),
                            1,
                            job.settings.required,
                        ));
                    }
                }
                Err(error) => {
                    for query in batch {
                        result.operations.push(operation(
                            &query.name,
                            Err(&error),
                            1,
                            job.settings.required,
                        ));
                    }
                }
            }
        }
    }
    if result.operations.is_empty() {
        result.operations.push(operation(
            "metrics-not-found",
            Err(&Error::Missing),
            0,
            job.settings.required,
        ));
    }
    result.finished_at = Utc::now();
    result
}
fn aggregation(value: Aggregation) -> (&'static str, &'static str) {
    match value {
        Aggregation::Minimum => ("Minimum", "/minimum"),
        Aggregation::Maximum => ("Maximum", "/maximum"),
        Aggregation::Average | Aggregation::Latest => ("Average", "/average"),
        Aggregation::Sum => ("Total", "/total"),
    }
}
fn endpoint(job: &Job, batch: &[&MetricQuery]) -> Result<Endpoint, Error> {
    let query = batch.first().ok_or(Error::Missing)?;
    if !crate::azure_metric_discovery::valid_resource(job, &query.resource) {
        return Err(Error::Forbidden);
    }
    let now = Utc::now();
    let mut url = url::Url::parse(&format!(
        "https://management.azure.com{}/providers/Microsoft.Insights/metrics",
        query.resource
    ))
    .map_err(|_| Error::Malformed)?;
    let names = batch
        .iter()
        .map(|query| query.metric.as_str())
        .collect::<Vec<_>>()
        .join(",");
    url.query_pairs_mut()
        .append_pair("api-version", "2023-10-01")
        .append_pair("metricnames", &names)
        .append_pair("metricnamespace", &query.namespace)
        .append_pair(
            "timespan",
            &format!(
                "{}/{}",
                (now - Duration::seconds(job.settings.metric_window.0 as i64))
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            ),
        )
        .append_pair("interval", "PT1M")
        .append_pair("aggregation", aggregation(query.aggregation).0)
        .append_pair("top", &job.settings.max_series.to_string());
    if !query.dimensions.is_empty() {
        let filter = query
            .dimensions
            .iter()
            .map(|(key, value)| {
                format!(
                    "{} eq '{}'",
                    key.replace('\'', "''"),
                    value.replace('\'', "''")
                )
            })
            .collect::<Vec<_>>()
            .join(" and ");
        url.query_pairs_mut().append_pair("$filter", &filter);
    }
    Ok(Endpoint::get("azure-metric-batch", url.as_str(), "/value"))
}
fn project(
    job: &Job,
    query: &MetricQuery,
    payload: &Value,
    limit: usize,
) -> (Vec<Observation>, Result<usize, Error>) {
    let mut out = Vec::new();
    let outcome = (|| {
        let metrics = payload
            .get("value")
            .and_then(Value::as_array)
            .ok_or(Error::Malformed)?;
        let row = metrics
            .iter()
            .find(|row| text(row, &["/name/value"]) == Some(query.metric.as_str()))
            .ok_or(Error::Missing)?;
        if let Some(code) = text(row, &["/errorCode"])
            && code != "Success"
        {
            return Err(Error::Unavailable);
        }
        let series = row
            .get("timeseries")
            .and_then(Value::as_array)
            .ok_or(Error::Malformed)?;
        let mut missing = false;
        for series in series.iter().take(limit) {
            let points = series
                .get("data")
                .and_then(Value::as_array)
                .ok_or(Error::Malformed)?;
            if points.len() > job.settings.max_series.saturating_mul(1440).min(100_000) {
                return Err(Error::Limit);
            }
            let points = points
                .iter()
                .filter_map(|point| {
                    Some((
                        timestamp(point, &["/timeStamp"])?,
                        number(point, &[aggregation(query.aggregation).1])?,
                    ))
                })
                .collect();
            let mut dimensions: Vec<_> = series
                .get("metadatavalues")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|value| {
                    Some((text(value, &["/name/value"])?, text(value, &["/value"])?))
                })
                .collect();
            dimensions.sort_unstable();
            let id = crate::metric_window::id(&json!([query.resource, query.metric, dimensions]));
            match crate::metric_window::project(job, query, &id, points) {
                Ok(observation) => out.push(observation),
                Err(_) => missing = true,
            }
        }
        if series.len() > limit {
            Err(Error::Limit)
        } else if out.is_empty() || missing {
            Err(Error::Missing)
        } else {
            Ok(out.len())
        }
    })();
    (out, outcome)
}

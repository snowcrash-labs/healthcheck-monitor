//! Deterministic metric identities and explicit window aggregation.
use chrono::{DateTime, Utc};
use monitor_core::{
    config::{
        resolve::Job,
        types::{Aggregation, MetricQuery},
    },
    model::{Data, Observation},
};
use monitor_integrations::{
    projection::{identity, observation},
    transport::Error,
};
use sha2::{Digest, Sha256};
pub fn id(value: &serde_json::Value) -> String {
    let encoded = serde_json::to_vec(value).unwrap_or_default();
    Sha256::digest(encoded)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn project(
    job: &Job,
    query: &MetricQuery,
    series: &str,
    mut points: Vec<(DateTime<Utc>, f64)>,
) -> Result<Observation, Error> {
    points.retain(|(_, value)| value.is_finite());
    points.sort_unstable_by_key(|(at, _)| *at);
    points.dedup_by_key(|(at, _)| *at);
    let Some((newest, latest)) = points.last().copied() else {
        return Err(Error::Missing);
    };
    let oldest = points.first().map_or(newest, |(at, _)| *at);
    let value = match query.aggregation {
        Aggregation::Minimum => points
            .iter()
            .map(|(_, v)| *v)
            .reduce(f64::min)
            .ok_or(Error::Missing)?,
        Aggregation::Maximum => points
            .iter()
            .map(|(_, v)| *v)
            .reduce(f64::max)
            .ok_or(Error::Missing)?,
        Aggregation::Average => points.iter().map(|(_, v)| v).sum::<f64>() / points.len() as f64,
        Aggregation::Sum => points.iter().map(|(_, v)| v).sum(),
        Aggregation::Latest => latest,
    };
    let continuous = points
        .windows(2)
        .all(|pair| (pair[1].0 - pair[0].0).num_seconds() <= 120);
    if !value.is_finite() {
        return Err(Error::Malformed);
    }
    let window_seconds = if continuous && matches!(query.aggregation, Aggregation::Minimum) {
        (newest - oldest).num_seconds().max(0) as u64
    } else {
        0
    };
    let mut result = observation(
        job,
        &query.name,
        &format!("{}/{}", identity(series), query.resource),
        Data::Metric {
            name: query.name.clone(),
            value,
            capacity: query.capacity,
            warning: query.warning,
            error: query.error,
            window_seconds,
        },
    );
    result.observed_at = newest;
    Ok(result)
}

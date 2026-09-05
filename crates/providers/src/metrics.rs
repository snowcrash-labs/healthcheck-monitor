//! Operational metric windows, with explicit missing-series coverage.
use crate::{
    auth::Auth,
    common::{self, Endpoint},
};
use chrono::{Duration, Utc};
use monitor_core::{
    config::{resolve::Job, types::MetricQuery},
    model::*,
};
use monitor_integrations::{
    projection::{number, observation, operation, timestamp},
    transport::{Error, Http},
};
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
pub(crate) fn metric(query: &MetricQuery, value: f64, window_seconds: u64) -> Data {
    Data::Metric {
        name: query.name.clone(),
        value,
        capacity: query.capacity,
        warning: query.warning,
        error: query.error,
        window_seconds,
    }
}
pub async fn gcp(http: &Http, auth: &Auth, job: &Job, cancel: &CancellationToken) -> CheckResult {
    let mut result = empty(job);
    for query in &job.target.metrics {
        let now = Utc::now();
        let mut url = match url::Url::parse(&format!(
            "https://monitoring.googleapis.com/v3/projects/{}/timeSeries",
            job.target.scope
        )) {
            Ok(url) => url,
            Err(_) => continue,
        };
        let mut filter = format!(
            "metric.type=\"{}\" AND resource.type=\"{}\"",
            query.metric, query.namespace
        );
        for (key, value) in &query.dimensions {
            let quoted = serde_json::to_string(value).unwrap_or_default();
            filter.push_str(&format!(" AND resource.labels.{key}={quoted}"));
        }
        url.query_pairs_mut()
            .append_pair("filter", &filter)
            .append_pair(
                "interval.startTime",
                &(now - Duration::seconds(job.settings.metric_window.0 as i64)).to_rfc3339(),
            )
            .append_pair("interval.endTime", &now.to_rfc3339())
            .append_pair("pageSize", &job.settings.max_series.min(1000).to_string())
            .append_pair("view", "FULL");
        let endpoint = Endpoint::get(&query.name, url.as_str(), "/timeSeries");
        let outcome = match common::request(http, auth, &endpoint, job, cancel).await {
            Ok(value) => match serde_json::from_value::<
                google_cloud_monitoring_v3::model::ListTimeSeriesResponse,
            >(value)
            {
                Ok(response) => {
                    let mut count = 0;
                    for row in response.time_series.iter().take(job.settings.max_series) {
                        let series = crate::metric_window::id(
                            &serde_json::json!({"resource":row.resource,"metric":row.metric}),
                        );
                        let points = row
                            .points
                            .iter()
                            .filter_map(|point| {
                                let time = point.interval.as_ref()?.end_time.as_ref()?;
                                let time = chrono::DateTime::from_timestamp(time.seconds(), 0)?;
                                let value = point.value.as_ref()?;
                                let number = value
                                    .double_value()
                                    .copied()
                                    .or_else(|| value.int64_value().map(|v| *v as f64))
                                    .or_else(|| value.distribution_value().map(|d| d.mean))?;
                                Some((time, number))
                            })
                            .collect();
                        if let Ok(observation) =
                            crate::metric_window::project(job, query, &series, points)
                        {
                            result.observations.push(observation);
                            count += 1;
                        }
                    }
                    if !response.next_page_token.is_empty() {
                        Err(Error::Limit)
                    } else if count == 0 {
                        Err(Error::Missing)
                    } else {
                        Ok(count)
                    }
                }
                Err(_) => Err(Error::Malformed),
            },
            Err(error) => Err(error),
        };
        result
            .operations
            .push(operation(&query.name, outcome.as_ref().copied(), 1, true));
    }
    result.finished_at = Utc::now();
    result
}
pub async fn azure(http: &Http, auth: &Auth, job: &Job, cancel: &CancellationToken) -> CheckResult {
    let mut result = empty(job);
    for query in &job.target.metrics {
        let now = Utc::now();
        let mut url = match url::Url::parse(&format!(
            "https://management.azure.com{}/providers/Microsoft.Insights/metrics",
            query.resource
        )) {
            Ok(u) => u,
            Err(_) => continue,
        };
        url.query_pairs_mut()
            .append_pair("api-version", "2023-10-01")
            .append_pair("metricnames", &query.metric)
            .append_pair("metricnamespace", &query.namespace)
            .append_pair(
                "timespan",
                &format!(
                    "{}/{}",
                    (now - Duration::seconds(job.settings.metric_window.0 as i64)).to_rfc3339(),
                    now.to_rfc3339()
                ),
            )
            .append_pair("interval", "PT1M")
            .append_pair("aggregation", "Minimum");
        let endpoint = Endpoint::get(&query.name, url.as_str(), "/value");
        let outcome = match common::request(http, auth, &endpoint, job, cancel).await {
            Ok(value) => {
                let mut count = 0;
                if let Some(metrics) = value.get("value").and_then(|v| v.as_array()) {
                    for row in metrics {
                        if let Some(series) = row.get("timeseries").and_then(|v| v.as_array()) {
                            for (i, series) in
                                series.iter().take(job.settings.max_series).enumerate()
                            {
                                if let Some(points) = series.get("data").and_then(|v| v.as_array())
                                {
                                    let minimum = points
                                        .iter()
                                        .filter_map(|p| number(p, &["/minimum"]))
                                        .reduce(f64::min);
                                    if let Some(value) = minimum {
                                        let oldest = points
                                            .iter()
                                            .filter_map(|p| timestamp(p, &["/timeStamp"]))
                                            .min();
                                        let newest = points
                                            .iter()
                                            .filter_map(|p| timestamp(p, &["/timeStamp"]))
                                            .max();
                                        let mut obs = observation(
                                            job,
                                            &query.name,
                                            &format!("{}/{i}", query.resource),
                                            metric(
                                                query,
                                                value,
                                                oldest.zip(newest).map_or(0, |(a, b)| {
                                                    (b - a).num_seconds().max(0) as u64
                                                }),
                                            ),
                                        );
                                        if let Some(time) = newest {
                                            obs.observed_at = time;
                                        }
                                        result.observations.push(obs);
                                        count += 1;
                                    }
                                }
                            }
                        }
                    }
                }
                if count == 0 {
                    Err(Error::Unavailable)
                } else {
                    Ok(count)
                }
            }
            Err(e) => Err(e),
        };
        result
            .operations
            .push(operation(&query.name, outcome.as_ref().copied(), 1, true));
    }
    result.finished_at = Utc::now();
    result
}
pub use crate::aws_metrics::aws;

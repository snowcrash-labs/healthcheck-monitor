//! Combine bounded Monitoring pages before evaluating a complete metric window.
use crate::common::{Endpoint, Source};
use chrono::{DateTime, Duration, Utc};
use monitor_core::{
    config::{resolve::Job, types::MetricQuery},
    model::Observation,
};
use monitor_integrations::transport::Error;
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;
struct Series {
    labels: String,
    points: Vec<(DateTime<Utc>, f64)>,
}
pub async fn collect<S: Source>(
    source: &S,
    job: &Job,
    query: &MetricQuery,
    limit: usize,
    cancel: &CancellationToken,
) -> (Vec<Observation>, Result<usize, Error>, usize) {
    if limit == 0 {
        return (vec![], Err(Error::Limit), 0);
    }
    let Ok(mut url) = url::Url::parse(&format!(
        "https://monitoring.googleapis.com/v3/projects/{}/timeSeries",
        job.target.scope
    )) else {
        return (vec![], Err(Error::Malformed), 0);
    };
    let now = Utc::now();
    let quoted = |value: &str| serde_json::to_string(value).unwrap_or_default();
    let mut filter = format!(
        "metric.type={} AND resource.type={}",
        quoted(&query.metric),
        quoted(&query.namespace)
    );
    for (key, value) in &query.dimensions {
        filter.push_str(&format!(" AND resource.labels.{key}={}", quoted(value)));
    }
    url.query_pairs_mut()
        .append_pair("filter", &filter)
        .append_pair(
            "interval.startTime",
            &(now - Duration::seconds(job.settings.metric_window.0 as i64)).to_rfc3339(),
        )
        .append_pair("interval.endTime", &now.to_rfc3339())
        .append_pair("pageSize", &limit.min(job.settings.page_size).to_string())
        .append_pair("view", "FULL");
    let mut series: BTreeMap<String, Series> = BTreeMap::new();
    let mut pages = 0;
    let mut status = Ok(());
    let mut token = String::new();
    let mut points = 0usize;
    let point_limit = (job.settings.response_bytes / 64).max(1);
    for page in 0..job.settings.max_pages {
        pages = page + 1;
        let endpoint = Endpoint::get(&query.name, url.as_str(), "/timeSeries");
        let response = match source
            .request(&endpoint, job, cancel)
            .await
            .and_then(|value| {
                serde_json::from_value::<google_cloud_monitoring_v3::model::ListTimeSeriesResponse>(
                    value,
                )
                .map_err(|_| Error::Malformed)
            }) {
            Ok(response) => response,
            Err(error) => {
                status = Err(error);
                break;
            }
        };
        for row in response.time_series {
            let id = crate::metric_window::id(
                &serde_json::json!({"resource":row.resource,"metric":row.metric}),
            );
            let labels = crate::metric_identity::label_path(
                row.metric
                    .iter()
                    .flat_map(|metric| metric.labels.iter())
                    .chain(
                        row.resource
                            .iter()
                            .flat_map(|resource| resource.labels.iter()),
                    )
                    .map(|(key, value)| (key.as_str(), value.as_str())),
            );
            if !job.target.resources.is_empty()
                && !job.target.resources.iter().any(|selector| {
                    labels.contains(selector)
                        || row
                            .resource
                            .iter()
                            .flat_map(|resource| resource.labels.values())
                            .any(|value| value.contains(selector))
                })
            {
                continue;
            }
            if series.len() >= limit && !series.contains_key(&id) {
                status = Err(Error::Limit);
                break;
            }
            let entry = series.entry(id).or_insert(Series {
                labels,
                points: Vec::new(),
            });
            for point in row.points {
                if points >= point_limit {
                    status = Err(Error::Limit);
                    break;
                }
                let parsed = (|| {
                    let time = point.interval.as_ref()?.end_time.as_ref()?;
                    let at = DateTime::from_timestamp(time.seconds(), 0)?;
                    let value = point.value.as_ref()?;
                    let value = value
                        .double_value()
                        .copied()
                        .or_else(|| value.int64_value().map(|value| *value as f64))
                        .or_else(|| {
                            value
                                .distribution_value()
                                .map(|distribution| distribution.mean)
                        })?;
                    value.is_finite().then_some((at, value))
                })();
                if let Some(point) = parsed {
                    entry.points.push(point);
                    points += 1;
                } else {
                    status = Err(Error::Missing);
                }
            }
        }
        if let Some(error) = response.execution_errors.first() {
            status = Err(match error.code {
                7 => Error::Denied,
                8 => Error::Throttled,
                16 => Error::Authentication,
                _ => Error::Unavailable,
            });
        }
        let next = response.next_page_token;
        if status.is_err() || next.is_empty() {
            break;
        }
        if token == next || pages == job.settings.max_pages {
            status = Err(Error::Limit);
            break;
        }
        token = next;
        let pairs: Vec<_> = url
            .query_pairs()
            .filter(|(key, _)| key != "pageToken")
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        url.query_pairs_mut()
            .clear()
            .extend_pairs(pairs)
            .append_pair("pageToken", &token);
    }
    let mut observations = Vec::new();
    for (id, series) in series {
        match crate::metric_window::project(
            job,
            query,
            &format!("{id}/{}", series.labels),
            series.points,
        ) {
            Ok(observation) => observations.push(observation),
            Err(error) => status = Err(error),
        }
    }
    if observations.is_empty() && status.is_ok() {
        status = Err(Error::Missing);
    }
    let count = observations.len();
    (observations, status.map(|()| count), pages)
}

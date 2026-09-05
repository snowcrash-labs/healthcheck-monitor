//! Bounded CloudWatch pages retain a result for every requested metric.
use aws_sdk_cloudwatch::{
    error::ProvideErrorMetadata,
    types::{Dimension, Metric, MetricDataQuery, MetricDataResult, MetricStat},
};
use chrono::{DateTime, Utc};
use monitor_core::{
    config::{
        resolve::Job,
        types::{Aggregation, MetricQuery},
    },
    model::*,
};
use monitor_integrations::{projection::operation, transport::Error};
use tokio_util::sync::CancellationToken;
pub struct Series {
    pub points: Vec<(DateTime<Utc>, f64)>,
    pub coverage: Coverage,
}
impl Default for Series {
    fn default() -> Self {
        Self {
            points: vec![],
            coverage: Coverage::Missing,
        }
    }
}
impl Series {
    pub fn record(&mut self, row: &MetricDataResult, limit: usize) {
        if !matches!(
            self.coverage,
            Coverage::Truncated | Coverage::Malformed | Coverage::Denied
        ) {
            self.coverage = match row.status_code().map(|code| code.as_str()) {
                Some("Complete") => Coverage::Complete,
                Some("Forbidden") => Coverage::Denied,
                Some("InternalError") => Coverage::Unavailable,
                _ => Coverage::Missing,
            };
        }
        if row.timestamps().len() != row.values().len() {
            self.coverage = Coverage::Malformed;
        }
        for (time, value) in row.timestamps().iter().zip(row.values()) {
            if self.points.len() >= limit {
                self.coverage = Coverage::Truncated;
                break;
            }
            if let Some(time) = DateTime::from_timestamp(time.secs(), 0)
                && value.is_finite()
            {
                self.points.push((time, *value));
            } else {
                self.coverage = Coverage::Malformed;
            }
        }
    }
}
pub async fn collect(
    client: &aws_sdk_cloudwatch::Client,
    job: &Job,
    region: &str,
    batch: &[MetricQuery],
    cancel: &CancellationToken,
) -> CheckResult {
    let mut result = crate::router::base(job);
    let mut series: Vec<Series> = (0..batch.len()).map(|_| Series::default()).collect();
    let queries: Vec<_> = batch
        .iter()
        .enumerate()
        .map(|(index, query)| {
            let dimensions = query
                .dimensions
                .iter()
                .map(|(key, value)| Dimension::builder().name(key).value(value).build())
                .collect();
            MetricDataQuery::builder()
                .id(format!("m{index}"))
                .return_data(true)
                .metric_stat(
                    MetricStat::builder()
                        .metric(
                            Metric::builder()
                                .namespace(&query.namespace)
                                .metric_name(&query.metric)
                                .set_dimensions(Some(dimensions))
                                .build(),
                        )
                        .period(60)
                        .stat(match query.aggregation {
                            Aggregation::Minimum => "Minimum",
                            Aggregation::Maximum => "Maximum",
                            Aggregation::Sum => "Sum",
                            _ => "Average",
                        })
                        .build(),
                )
                .build()
        })
        .collect();
    let now = Utc::now().timestamp();
    let point_limit = (job.settings.response_bytes / 64 / batch.len().max(1)).max(1);
    let mut token = None;
    let mut pages = 0;
    let mut fault = None;
    for page in 0..job.settings.max_pages {
        pages = page + 1;
        let response = tokio::select! {
            _=cancel.cancelled()=>Err(Error::Cancelled),
            response=client.get_metric_data().set_metric_data_queries(Some(queries.clone())).start_time(aws_smithy_types::DateTime::from_secs(now-job.settings.metric_window.0 as i64)).end_time(aws_smithy_types::DateTime::from_secs(now)).max_datapoints((point_limit*batch.len()).min(100800) as i32).set_next_token(token.clone()).send()=>response.map_err(|error|crate::aws_errors::classify(error.as_service_error().and_then(|error|error.code()))),
        };
        match response {
            Ok(response) => {
                for row in response.metric_data_results() {
                    let index = row
                        .id()
                        .and_then(|id| id.strip_prefix('m'))
                        .and_then(|id| id.parse::<usize>().ok());
                    if let Some(series) = index.and_then(|index| series.get_mut(index)) {
                        series.record(row, point_limit);
                    } else {
                        result.operations.push(operation(
                            &format!("cloudwatch-batch/{region}/unexpected-series"),
                            Err(&Error::Malformed),
                            pages,
                            true,
                        ));
                    }
                }
                let next = response
                    .next_token()
                    .filter(|token| !token.is_empty())
                    .map(String::from);
                if next.is_none() {
                    break;
                }
                if next == token
                    || pages == job.settings.max_pages
                    || series
                        .iter()
                        .all(|series| series.coverage == Coverage::Truncated)
                {
                    fault = Some(Coverage::Truncated);
                    break;
                }
                token = next;
            }
            Err(error) => {
                fault = Some(error.coverage());
                break;
            }
        }
    }
    for (query, series) in batch.iter().zip(series) {
        let id = format!("{region}/{}", query.name);
        let mut coverage = fault.unwrap_or(series.coverage);
        let outcome = crate::metric_window::project(
            job,
            query,
            &format!("{region}/{}", query.resource),
            series.points,
        );
        let count = match outcome {
            Ok(mut observation) => {
                observation.operation = id.clone();
                result.observations.push(observation);
                1
            }
            Err(error) => {
                if coverage == Coverage::Complete {
                    coverage = error.coverage();
                }
                0
            }
        };
        let mut operation = operation(&id, Ok(count), pages, job.settings.required);
        operation.coverage = coverage;
        result.operations.push(operation);
    }
    result.finished_at = Utc::now();
    result
}

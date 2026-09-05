//! Native CloudWatch metric batches.
use crate::{
    auth::Auth,
    metrics::{empty, metric},
};
use chrono::Utc;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{
    projection::{observation, operation},
    transport::Error,
};
use tokio_util::sync::CancellationToken;
pub async fn aws(auth: &Auth, job: &Job, cancel: &CancellationToken) -> CheckResult {
    use aws_sdk_cloudwatch::{
        primitives::DateTime,
        types::{Dimension, Metric, MetricDataQuery, MetricStat},
    };
    let mut result = empty(job);
    let Auth::Aws(config) = auth else {
        return result;
    };
    let client = aws_sdk_cloudwatch::Client::new(config);
    for batch in job.target.metrics.chunks(500) {
        if cancel.is_cancelled() {
            break;
        }
        let mut queries = Vec::new();
        for (i, query) in batch.iter().enumerate() {
            let dimensions = query
                .dimensions
                .iter()
                .map(|(k, v)| Dimension::builder().name(k).value(v).build())
                .collect::<Vec<_>>();
            queries.push(
                MetricDataQuery::builder()
                    .id(format!("m{i}"))
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
                            .stat("Minimum")
                            .build(),
                    )
                    .return_data(true)
                    .build(),
            );
        }
        let now = Utc::now().timestamp();
        let response = client
            .get_metric_data()
            .set_metric_data_queries(Some(queries))
            .start_time(DateTime::from_secs(
                now - job.settings.metric_window.0 as i64,
            ))
            .end_time(DateTime::from_secs(now))
            .max_datapoints((job.settings.max_series * 60).min(100800) as i32)
            .send()
            .await;
        match response {
            Ok(response) => {
                for row in response.metric_data_results() {
                    let index = row
                        .id()
                        .and_then(|s| s.strip_prefix('m'))
                        .and_then(|s| s.parse::<usize>().ok());
                    if let Some(query) = index.and_then(|i| batch.get(i)) {
                        let minimum = row.values().iter().copied().reduce(f64::min);
                        let outcome = if let Some(value) = minimum {
                            let oldest = row.timestamps().iter().map(|t| t.secs()).min();
                            let newest = row.timestamps().iter().map(|t| t.secs()).max();
                            let mut obs = observation(
                                job,
                                &query.name,
                                &query.resource,
                                metric(
                                    query,
                                    value,
                                    oldest
                                        .zip(newest)
                                        .map_or(0, |(a, b)| b.saturating_sub(a) as u64),
                                ),
                            );
                            if let Some(time) =
                                newest.and_then(|t| chrono::DateTime::from_timestamp(t, 0))
                            {
                                obs.observed_at = time;
                            }
                            result.observations.push(obs);
                            if response.next_token().is_some() {
                                Err(Error::Limit)
                            } else {
                                Ok(1)
                            }
                        } else {
                            Err(Error::Unavailable)
                        };
                        result.operations.push(operation(
                            &query.name,
                            outcome.as_ref().copied(),
                            1,
                            true,
                        ));
                    }
                }
            }
            Err(_) => result.operations.push(operation(
                "cloudwatch-batch",
                Err(&Error::Unavailable),
                1,
                true,
            )),
        }
    }
    result.finished_at = Utc::now();
    result
}

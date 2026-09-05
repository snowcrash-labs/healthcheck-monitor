//! Native CloudWatch metric batches.
use crate::{auth::Auth, metrics::empty};
use chrono::Utc;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{projection::operation, transport::Error};
use tokio_util::sync::CancellationToken;
pub async fn aws(auth: &Auth, job: &Job, cancel: &CancellationToken) -> CheckResult {
    use aws_sdk_cloudwatch::{
        primitives::DateTime,
        types::{Dimension, Metric, MetricDataQuery, MetricStat},
    };
    let mut result = empty(job);
    let Auth::Aws(clients) = auth else {
        return result;
    };
    result.operations.clear();
    for region in &job.target.regions {
        let client = match clients.cloudwatch(region).await {
            Ok(client) => client,
            Err(error) => {
                result
                    .operations
                    .push(operation("cloudwatch-client", Err(&error), 0, true));
                continue;
            }
        };
        let queries = if job.target.metrics.is_empty() {
            let (queries, operations) = crate::aws_metric_discovery::discover(&client, job).await;
            result.operations.extend(operations);
            queries
        } else {
            job.target.metrics.clone()
        };
        for batch in queries.chunks(500) {
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
                                .stat(match query.aggregation {
                                    monitor_core::config::types::Aggregation::Minimum => "Minimum",
                                    monitor_core::config::types::Aggregation::Maximum => "Maximum",
                                    monitor_core::config::types::Aggregation::Sum => "Sum",
                                    _ => "Average",
                                })
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
                            let points = row
                                .timestamps()
                                .iter()
                                .zip(row.values())
                                .filter_map(|(time, value)| {
                                    chrono::DateTime::from_timestamp(time.secs(), 0)
                                        .map(|at| (at, *value))
                                })
                                .collect();
                            let operation_id = format!("{region}/{}", query.name);
                            let outcome = match crate::metric_window::project(
                                job,
                                query,
                                &format!("{region}/{}", query.resource),
                                points,
                            ) {
                                Ok(mut observation) => {
                                    observation.operation = operation_id.clone();
                                    result.observations.push(observation);
                                    if response.next_token().is_some() {
                                        Err(Error::Limit)
                                    } else if row
                                        .status_code()
                                        .is_none_or(|code| code.as_str() != "Complete")
                                    {
                                        Err(Error::Missing)
                                    } else {
                                        Ok(1)
                                    }
                                }
                                Err(error) => Err(error),
                            };
                            result.operations.push(operation(
                                &operation_id,
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
    }
    if result.operations.is_empty() {
        result.operations.push(operation(
            "cloudwatch-metrics",
            Err(&Error::Missing),
            0,
            true,
        ));
    }
    result.finished_at = Utc::now();
    result
}

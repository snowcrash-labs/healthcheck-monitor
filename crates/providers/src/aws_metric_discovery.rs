//! Discover bounded CloudWatch dimensions before issuing batched metric reads.
use aws_sdk_cloudwatch::error::ProvideErrorMetadata;
use monitor_core::{
    config::{resolve::Job, types::MetricQuery},
    model::*,
};
use monitor_integrations::{projection::operation, transport::Error};
use std::collections::BTreeMap;
pub async fn discover(
    client: &aws_sdk_cloudwatch::Client,
    job: &Job,
    region: &str,
    expected: Option<&std::collections::BTreeSet<String>>,
) -> (Vec<MetricQuery>, Vec<Operation>) {
    let mut metrics = Vec::new();
    let mut operations = Vec::new();
    let namespaces: Vec<_> = crate::metric_catalog::AWS_NAMESPACES
        .iter()
        .filter(|namespace| {
            expected.is_none_or(|expected| expected.contains(**namespace))
                && crate::aws_metric_plan::namespace_in_region(job, namespace, region)
                && (job.check != Check::Queues
                    || matches!(**namespace, "AWS/SQS" | "AWS/SNS" | "AWS/Events"))
        })
        .collect();
    for (index, namespace) in namespaces.iter().enumerate() {
        let allowance =
            job.settings.max_series.saturating_sub(metrics.len()) / (namespaces.len() - index);
        let mut token = None;
        let mut count = 0;
        let mut coverage = Ok(0);
        let mut pages = 0;
        for _ in 0..job.settings.max_pages {
            pages += 1;
            match client
                .list_metrics()
                .namespace(**namespace)
                .set_next_token(token.clone())
                .send()
                .await
            {
                Ok(response) => {
                    for metric in response.metrics() {
                        if count >= allowance {
                            coverage = Err(Error::Limit);
                            break;
                        }
                        let Some(name) = metric.metric_name() else {
                            continue;
                        };
                        let dimensions: BTreeMap<String, String> = metric
                            .dimensions()
                            .iter()
                            .filter_map(|d| d.name().zip(d.value()))
                            .map(|(k, v)| (k.into(), v.into()))
                            .collect();
                        if !job.target.resources.is_empty()
                            && !job.target.resources.iter().any(|wanted| {
                                dimensions
                                    .values()
                                    .any(|value: &String| value.contains(wanted))
                            })
                        {
                            continue;
                        }
                        let suffix = crate::metric_window::id(
                            &serde_json::json!({"namespace":namespace,"metric":name,"dimensions":dimensions}),
                        );
                        let labels = crate::metric_identity::label_path(
                            dimensions
                                .iter()
                                .map(|(key, value)| (key.as_str(), value.as_str())),
                        );
                        metrics.push(MetricQuery {
                            aggregation: crate::aws_metric_plan::aggregation(namespace, name),
                            name: format!("{namespace}/{name}/{suffix}"),
                            namespace: (**namespace).into(),
                            metric: name.into(),
                            resource: format!("{namespace}/{name}/{suffix}/{labels}"),
                            dimensions,
                            capacity: crate::metric_catalog::percent_metric(name).then_some(100.0),
                            warning: None,
                            error: (name == "ApproximateAgeOfOldestMessage")
                                .then_some(job.settings.queue_age_error)
                                .flatten(),
                        });
                        count += 1;
                    }
                    if coverage.is_err() {
                        break;
                    }
                    let next = response
                        .next_token()
                        .filter(|token| !token.is_empty())
                        .map(String::from);
                    if next.is_some() && next == token {
                        coverage = Err(Error::Limit);
                        break;
                    }
                    token = next;
                    if token.is_none() {
                        break;
                    }
                    if pages == job.settings.max_pages {
                        coverage = Err(Error::Limit);
                    }
                }
                Err(error) => {
                    coverage = Err(crate::aws_errors::classify(
                        error.as_service_error().and_then(|error| error.code()),
                    ));
                    break;
                }
            }
        }
        if count == 0 && coverage.is_ok() && expected.is_some() {
            coverage = Err(Error::Missing);
        }
        operations.push(operation(
            &format!("metric-discovery/{region}/{namespace}"),
            coverage.map(|_| count).as_ref().copied(),
            pages,
            true,
        ));
    }
    (metrics, operations)
}

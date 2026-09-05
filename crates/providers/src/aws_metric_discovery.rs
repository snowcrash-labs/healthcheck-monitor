//! Discover bounded CloudWatch dimensions before issuing batched metric reads.
use monitor_core::{
    config::{resolve::Job, types::MetricQuery},
    model::*,
};
use monitor_integrations::{projection::operation, transport::Error};
use std::collections::BTreeMap;
pub async fn discover(
    client: &aws_sdk_cloudwatch::Client,
    job: &Job,
) -> (Vec<MetricQuery>, Vec<Operation>) {
    let mut metrics = Vec::new();
    let mut operations = Vec::new();
    for namespace in crate::metric_catalog::AWS_NAMESPACES {
        let mut token = None;
        let mut count = 0;
        let mut coverage = Ok(0);
        let mut pages = 0;
        for _ in 0..job.settings.max_pages {
            pages += 1;
            match client
                .list_metrics()
                .namespace(*namespace)
                .set_next_token(token)
                .send()
                .await
            {
                Ok(response) => {
                    for metric in response.metrics() {
                        if metrics.len() >= job.settings.max_series {
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
                            aggregation: Default::default(),
                            name: format!("{namespace}/{name}/{suffix}"),
                            namespace: (*namespace).into(),
                            metric: name.into(),
                            resource: format!("{namespace}/{name}/{suffix}/{labels}"),
                            dimensions,
                            capacity: crate::metric_catalog::percent_metric(name).then_some(100.0),
                            warning: None,
                            error: None,
                        });
                        count += 1;
                    }
                    if coverage.is_err() {
                        break;
                    }
                    token = response.next_token().map(String::from);
                    if token.is_none() {
                        break;
                    }
                    if pages == job.settings.max_pages {
                        coverage = Err(Error::Limit);
                    }
                }
                Err(_) => {
                    coverage = Err(Error::Unavailable);
                    break;
                }
            }
        }
        operations.push(operation(
            &format!("metric-discovery/{namespace}"),
            coverage.map(|_| count).as_ref().copied(),
            pages,
            true,
        ));
        if metrics.len() >= job.settings.max_series {
            break;
        }
    }
    (metrics, operations)
}

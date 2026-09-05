//! Resource Graph inventory and bounded Azure metric-definition discovery.
use crate::common::{Endpoint, Source};
use monitor_core::{
    config::{
        resolve::Job,
        types::{Aggregation, MetricQuery},
    },
    model::*,
};
use monitor_integrations::{
    projection::{operation, text},
    transport::Error,
};
use serde_json::Value;
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

/// ARM resource identities stay within the selected subscription and never carry query syntax.
pub fn valid_resource(job: &Job, resource: &str) -> bool {
    resource.len() <= 2048
        && resource.to_ascii_lowercase().starts_with(&format!(
            "/subscriptions/{}/",
            job.target.scope.to_ascii_lowercase()
        ))
        && !resource.contains(['?', '#', '%', '\\'])
        && !resource.split('/').any(|part| part == ".." || part == ".")
        && !resource.chars().any(char::is_control)
}
pub fn supported(namespace: &str) -> bool {
    let namespace = namespace.to_ascii_lowercase();
    [
        "microsoft.compute/",
        "microsoft.containerservice/",
        "microsoft.app/",
        "microsoft.web/",
        "microsoft.network/",
        "microsoft.cdn/",
        "microsoft.sql/",
        "microsoft.dbforpostgresql/",
        "microsoft.cache/",
        "microsoft.documentdb/",
        "microsoft.storage/",
        "microsoft.servicebus/",
        "microsoft.eventhub/",
        "microsoft.eventgrid/",
        "microsoft.containerregistry/",
        "microsoft.keyvault/",
        "microsoft.recoveryservices/",
    ]
    .iter()
    .any(|prefix| namespace.starts_with(prefix))
}
pub async fn discover<S: Source>(
    source: &S,
    job: &Job,
    cancel: &CancellationToken,
) -> (Vec<MetricQuery>, CheckResult) {
    let mut result =
        crate::common::collect_from(source, job, vec![crate::azure::graph(job)], cancel).await;
    let resources: Vec<_> = result
        .observations
        .iter()
        .filter_map(|observation| match &observation.data {
            Data::MetricResource {
                resource_id,
                namespace,
            } if job.check != Check::Queues
                || [
                    "microsoft.servicebus/",
                    "microsoft.eventhub/",
                    "microsoft.eventgrid/",
                ]
                .iter()
                .any(|prefix| namespace.to_ascii_lowercase().starts_with(prefix)) =>
            {
                Some((resource_id.clone(), namespace.clone()))
            }
            _ => None,
        })
        .collect();
    let mut queries = Vec::new();
    let resource_count = resources.len().max(1);
    let per_resource = (job.settings.max_series / resource_count).clamp(1, 32);
    for (index, (resource, namespace)) in resources.into_iter().enumerate() {
        if queries.len() >= job.settings.max_series || index >= job.settings.max_series {
            result.operations.push(operation(
                "metric-discovery-limit",
                Err(&Error::Limit),
                0,
                true,
            ));
            break;
        }
        let id = format!(
            "metric-definitions/{}",
            crate::metric_window::id(&Value::String(resource.clone()))
        );
        let endpoint = Endpoint::get(
            &id,
            format!(
                "https://management.azure.com{resource}/providers/Microsoft.Insights/metricDefinitions?api-version=2023-10-01"
            ),
            "/value",
        );
        let outcome = match source.request(&endpoint, job, cancel).await {
            Ok(value) => match definitions(
                &value,
                &resource,
                &namespace,
                job.check == Check::Queues,
                per_resource.min(job.settings.max_series - queries.len()),
            ) {
                Ok((found, truncated)) => {
                    let count = found.len();
                    queries.extend(found);
                    if truncated {
                        Err(Error::Limit)
                    } else if count == 0 {
                        Err(Error::Missing)
                    } else {
                        Ok(count)
                    }
                }
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        };
        result.operations.push(operation(
            &id,
            outcome.as_ref().copied(),
            1,
            job.settings.required,
        ));
    }
    (queries, result)
}
fn relevant(name: &str, queues: bool) -> bool {
    let name = name.to_ascii_lowercase();
    let terms = if queues {
        &["message", "backlog", "lag", "deadletter", "throttl"][..]
    } else {
        &[
            "cpu",
            "memory",
            "error",
            "fail",
            "latency",
            "duration",
            "lag",
            "request",
            "connection",
            "evict",
            "message",
            "availability",
            "capacity",
            "percent",
            "utilization",
            "throttl",
            "health",
            "replica",
            "load",
        ][..]
    };
    terms.iter().any(|term| name.contains(term))
}
/// Provider definitions select legal aggregations; a percentage unit supplies a valid denominator.
fn definitions(
    value: &Value,
    resource: &str,
    namespace: &str,
    queues: bool,
    limit: usize,
) -> Result<(Vec<MetricQuery>, bool), Error> {
    let rows = value
        .get("value")
        .and_then(Value::as_array)
        .ok_or(Error::Malformed)?;
    let mut queries = Vec::new();
    let mut truncated = false;
    for row in rows {
        let name = text(row, &["/name/value"]).ok_or(Error::Malformed)?;
        if !relevant(name, queues) {
            continue;
        }
        if queries.len() >= limit {
            truncated = true;
            break;
        }
        if name.len() > 256 || name.contains(',') || name.chars().any(char::is_control) {
            return Err(Error::Malformed);
        }
        let percent =
            text(row, &["/unit"]) == Some("Percent") && crate::metric_catalog::percent_metric(name);
        let supports_min = row
            .get("supportedAggregationTypes")
            .and_then(Value::as_array)
            .is_some_and(|values| values.iter().any(|value| value.as_str() == Some("Minimum")));
        let aggregation = if percent && supports_min {
            Aggregation::Minimum
        } else {
            match text(row, &["/primaryAggregationType"]) {
                Some("Minimum") => Aggregation::Minimum,
                Some("Maximum") => Aggregation::Maximum,
                Some("Total") => Aggregation::Sum,
                Some("Average") => Aggregation::Average,
                _ => continue,
            }
        };
        queries.push(MetricQuery {
            aggregation,
            name: format!(
                "azure/{}",
                crate::metric_window::id(&serde_json::json!([resource, name]))
            ),
            namespace: namespace.into(),
            metric: name.into(),
            resource: resource.into(),
            dimensions: BTreeMap::new(),
            capacity: percent.then_some(100.0),
            warning: None,
            error: None,
        });
    }
    Ok((queries, truncated))
}

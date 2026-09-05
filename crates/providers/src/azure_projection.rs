//! Azure allowlisted resource and operational projections.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::projection::{self, observation, text};
use serde_json::Value;
pub fn graph(job: &Job, endpoint: &Endpoint, value: &Value) -> Vec<Observation> {
    let id = endpoint.id.as_str();
    let name = text(value, &["/id", "/name"]).unwrap_or("resource");
    let obs = |data| observation(job, id, name, data);
    if let (Some(resource_id), Some(namespace)) = (text(value, &["/id"]), text(value, &["/type"]))
        && crate::azure_metric_discovery::valid_resource(job, resource_id)
        && crate::azure_metric_discovery::supported(namespace)
    {
        return vec![observation(
            job,
            id,
            resource_id,
            Data::MetricResource {
                resource_id: resource_id.into(),
                namespace: namespace.into(),
            },
        )];
    }
    vec![obs(Data::Inventory {
        family: projection::identity(text(value, &["/type"]).unwrap_or("unknown")),
        supported: false,
    })]
}

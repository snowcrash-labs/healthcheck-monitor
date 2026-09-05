//! Azure allowlisted resource and operational projections.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::projection::{self, observation, text};
use serde_json::Value;
pub fn registry(job: &Job, endpoint: &Endpoint, value: &Value) -> Vec<Observation> {
    let Some(host) = text(value, &["/properties/loginServer"]).filter(|host| {
        host.ends_with(".azurecr.io")
            && host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-".contains(&byte))
    }) else {
        return vec![];
    };
    vec![observation(
        job,
        &endpoint.id,
        host,
        Data::Registry { host: host.into() },
    )]
}
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

pub fn workspace(job: &Job, endpoint: &Endpoint, value: &Value) -> Vec<Observation> {
    if let (Some(workspace_id), Some(resource_id)) = (
        text(value, &["/properties/customerId"]),
        text(value, &["/id"]),
    ) && workspace_id.len() == 36
        && workspace_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        && crate::azure_metric_discovery::valid_resource(job, resource_id)
    {
        return vec![observation(
            job,
            &endpoint.id,
            resource_id,
            Data::LogWorkspace {
                workspace_id: workspace_id.into(),
                resource_id: resource_id.into(),
            },
        )];
    }
    vec![]
}

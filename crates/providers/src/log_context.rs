//! Log attribution includes only explicit infrastructure labels, never diagnostic payloads.
use monitor_core::{config::resolve::Job, diagnostics::ResourceContext};
use monitor_integrations::{
    projection::text,
    resource_context::{base, identifier},
};
use serde_json::Value;
pub fn gcp(job: &Job, row: &Value, scope: &str) -> Option<ResourceContext> {
    let mut context = base(job, "logs", scope)?;
    context.namespace = text(row, &["/resource/labels/namespace_name"]).and_then(identifier);
    context.cluster = text(row, &["/resource/labels/cluster_name"]).and_then(identifier);
    context.region = text(row, &["/resource/labels/location"]).and_then(identifier);
    context.container = text(row, &["/resource/labels/container_name"]).and_then(identifier);
    context.name = text(
        row,
        &[
            "/resource/labels/pod_name",
            "/resource/labels/service_name",
            "/resource/labels/function_name",
        ],
    )
    .and_then(identifier);
    context.native_id = context.name.clone().unwrap_or_else(|| scope.into());
    Some(context)
}

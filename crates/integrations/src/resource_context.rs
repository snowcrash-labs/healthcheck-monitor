//! Location metadata is validated before it becomes evidence or a console-link input.
use super::projection::text;
use monitor_core::{config::resolve::Job, diagnostics::ResourceContext, model::Provider};
use serde_json::Value;

/// Preserve native identifiers exactly; reject control characters and overlong values.
pub fn identifier(value: &str) -> Option<String> {
    (!value.is_empty() && value.len() <= 2048 && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}
pub fn base(job: &Job, service: &str, native: &str) -> Option<ResourceContext> {
    let native_id = identifier(native)?;
    let region = if native.starts_with("arn:") {
        native
            .split(':')
            .nth(3)
            .filter(|s| !s.is_empty())
            .and_then(identifier)
    } else {
        let parts: Vec<_> = native.split('/').collect();
        parts
            .windows(2)
            .find(|pair| pair[0] == "locations" || pair[0] == "regions")
            .and_then(|pair| identifier(pair[1]))
    }
    .or_else(|| {
        if job.target.regions.len() == 1 {
            job.target.regions.first().cloned()
        } else {
            None
        }
    });
    Some(ResourceContext {
        provider: job.target.provider,
        scope: job.target.scope.clone(),
        service: service.split('/').next().unwrap_or(service).into(),
        native_id,
        region,
        zone: None,
        cluster: None,
        namespace: None,
        name: None,
        uid: None,
        container: None,
        reason: None,
        exit_code: None,
    })
}
pub fn kubernetes(job: &Job, kind: &str, value: &Value) -> Option<ResourceContext> {
    let name = text(value, &["/metadata/name"])?;
    let mut context = base(job, kind, name)?;
    context.name = identifier(name);
    context.namespace = text(value, &["/metadata/namespace"]).and_then(identifier);
    context.uid = text(value, &["/metadata/uid"]).and_then(identifier);
    context.cluster = None;
    // gcloud contexts explicitly encode project, location and cluster; other context names do not.
    if let Some(parts) = job
        .target
        .context
        .as_deref()
        .and_then(|value| value.strip_prefix("gke_"))
    {
        let parts: Vec<_> = parts.splitn(3, '_').collect();
        if let [project, location, cluster] = parts.as_slice() {
            context.provider = Provider::Gcp;
            context.scope = (*project).into();
            context.region = Some((*location).into());
            context.cluster = Some((*cluster).into());
        }
    }
    Some(context)
}

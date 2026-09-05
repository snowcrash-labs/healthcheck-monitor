//! Managed-runtime images retain references and resolved digests, never environment values.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, images, model::*};
use monitor_integrations::projection::{boolean, number, observation, text, timestamp};
use serde_json::Value;
pub fn project(job: &Job, endpoint: &Endpoint, value: &Value) -> Vec<Observation> {
    let family = endpoint.id.split('/').next().unwrap_or("");
    let mut out = Vec::new();
    let name = text(
        value,
        &["/name", "/taskArn", "/Configuration/FunctionName", "/id"],
    )
    .unwrap_or("runtime");
    if !job.target.resources.is_empty()
        && !job
            .target
            .resources
            .iter()
            .any(|selector| name.contains(selector) || endpoint.id.contains(selector))
    {
        return out;
    }
    if family == "lambda-image" {
        if let Some(desired) = text(value, &["/Code/ImageUri"]) {
            push(
                job,
                endpoint,
                name,
                "function",
                desired,
                text(value, &["/Code/ResolvedImageUri"]).and_then(images::digest),
                job.target.expected,
                &mut out,
            );
        }
        return out;
    }
    let path = match family {
        "cloud-run" if text(value, &["/latestReadyRevision"]).is_some() => return out,
        "cloud-run" => "/template/containers",
        "cloud-run-revision" => "/containers",
        "ecs-tasks-detail" => "/containers",
        "container-revisions" => "/properties/template/containers",
        _ => return out,
    };
    if family == "ecs-tasks-detail" {
        out.push(observation(
            job,
            &endpoint.id,
            name,
            Data::Workload {
                desired: 1,
                ready: u32::from(
                    text(value, &["/lastStatus"]) == Some("RUNNING")
                        && text(value, &["/healthStatus"]) != Some("UNHEALTHY"),
                ),
                created_at: timestamp(value, &["/createdAt"]),
                draining: text(value, &["/desiredStatus"]) == Some("STOPPED"),
                node: false,
            },
        ));
    }
    for (index, container) in value
        .pointer(path)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(job.settings.max_assets)
        .enumerate()
    {
        let Some(desired) = text(container, &["/image"]) else {
            continue;
        };
        let container_name = text(container, &["/name"])
            .map(String::from)
            .unwrap_or_else(|| format!("container-{index}"));
        let resolved = if family == "ecs-tasks-detail" {
            text(container, &["/imageDigest"]).and_then(images::digest)
        } else if family == "cloud-run-revision" {
            images::digest(desired).or_else(|| {
                text(value, &["/imageDigest", "/status/imageDigest"]).and_then(images::digest)
            })
        } else {
            None
        };
        let expected = if family == "container-revisions"
            && (boolean(value, &["/properties/active"]) == Some(false)
                || number(value, &["/properties/replicas"]) == Some(0.0)
                    && text(value, &["/properties/healthState"]) != Some("Unhealthy"))
            || family == "ecs-tasks-detail" && text(value, &["/desiredStatus"]) == Some("STOPPED")
        {
            Expected::ScaleToZero
        } else {
            job.target.expected
        };
        push(
            job,
            endpoint,
            name,
            &container_name,
            desired,
            resolved,
            expected,
            &mut out,
        );
    }
    out
}
#[allow(clippy::too_many_arguments)]
fn push(
    job: &Job,
    endpoint: &Endpoint,
    name: &str,
    container: &str,
    desired: &str,
    resolved: Option<&str>,
    expected: Expected,
    out: &mut Vec<Observation>,
) {
    let revision = images::tag(desired)
        .and_then(|tag| {
            tag.strip_prefix("release-")
                .or_else(|| tag.strip_prefix("sha-"))
        })
        .filter(|revision| {
            (7..=40).contains(&revision.len())
                && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        .map(String::from);
    let mut obs = observation(
        job,
        &endpoint.id,
        &format!("{name}/image/{container}"),
        Data::Image {
            desired: monitor_integrations::projection::identity(desired),
            observed_digest: resolved.map(String::from),
            revision,
        },
    );
    obs.expected = expected;
    out.push(obs);
}

//! Container image metadata keeps observed digests separate from desired references.
use super::projection::{observation, text};
use monitor_core::{config::resolve::Job, model::*};
use serde_json::Value;
pub fn images(job: &Job, kind: &str, name: &str, value: &Value, output: &mut Vec<Observation>) {
    let containers = [
        "/spec/containers",
        "/spec/template/spec/containers",
        "/spec/jobTemplate/spec/template/spec/containers",
    ]
    .into_iter()
    .find_map(|p| value.pointer(p).and_then(Value::as_array));
    if let Some(containers) = containers {
        for container in containers.iter().take(128) {
            let Some(image) = text(container, &["/image"]) else {
                continue;
            };
            let container_name = text(container, &["/name"]).unwrap_or("unknown");
            let digest = value
                .pointer("/status/containerStatuses")
                .and_then(Value::as_array)
                .and_then(|items| {
                    items
                        .iter()
                        .find(|s| text(s, &["/name"]) == Some(container_name))
                })
                .and_then(|v| text(v, &["/imageID"]));
            let (desired, observed_digest, revision) = image_parts(image, digest);
            output.push(observation(
                job,
                kind,
                &format!("{name}/image/{container_name}"),
                Data::Image {
                    desired,
                    observed_digest,
                    revision,
                },
            ));
        }
    }
}
pub fn image_parts(
    image: &str,
    observed: Option<&str>,
) -> (String, Option<String>, Option<String>) {
    let digest = observed
        .and_then(|s| s.split_once('@').map(|(_, d)| d))
        .map(super::projection::identity);
    let tag = image
        .rsplit('/')
        .next()
        .and_then(|s| s.split_once(':').map(|(_, t)| t))
        .unwrap_or("");
    let revision = tag
        .strip_prefix("release-")
        .or_else(|| tag.strip_prefix("sha-"))
        .filter(|s| (7..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(String::from);
    (super::projection::identity(image), digest, revision)
}

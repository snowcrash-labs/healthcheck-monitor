//! Registry and successful build metadata form independent provenance links.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, images, model::*};
use monitor_integrations::projection::{observation, text, timestamp};
use serde_json::Value;
pub fn manifest_kind(value: Option<&str>) -> ManifestKind {
    match value {
        Some(
            "application/vnd.oci.image.index.v1+json"
            | "application/vnd.docker.distribution.manifest.list.v2+json",
        ) => ManifestKind::Index,
        Some(
            "application/vnd.oci.image.manifest.v1+json"
            | "application/vnd.docker.distribution.manifest.v2+json",
        ) => ManifestKind::Image,
        _ => ManifestKind::Unknown,
    }
}
pub fn registry(job: &Job, endpoint: &Endpoint, value: &Value) -> Vec<Observation> {
    let reference = if job.target.provider == Provider::Gcp {
        text(value, &["/uri"]).map(String::from)
    } else {
        endpoint.aws.as_ref().and_then(|(_, region, _)| {
            text(value, &["/repositoryName"])
                .or_else(|| {
                    endpoint
                        .body
                        .as_ref()
                        .and_then(|body| text(body, &["/repositoryName"]))
                })
                .map(|repository| {
                    format!(
                        "{}.dkr.ecr.{region}.amazonaws.com/{repository}",
                        job.target.scope
                    )
                })
        })
    };
    let Some(reference) = reference else {
        return vec![];
    };
    let Some(image) = images::canonical(&reference) else {
        return vec![];
    };
    if !job.target.resources.is_empty()
        && !job
            .target
            .resources
            .iter()
            .any(|selector| image.contains(selector) || endpoint.id.contains(selector))
    {
        return vec![];
    }
    let Some(digest) = text(value, &["/imageDigest"])
        .and_then(images::digest)
        .or_else(|| images::digest(&reference))
    else {
        return vec![];
    };
    let tags = value
        .get("tags")
        .or_else(|| value.get("imageTags"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .take(32)
        .filter_map(|tag| {
            let tag = images::tag(tag).unwrap_or(tag);
            (tag.len() <= 128
                && tag
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte)))
            .then(|| tag.to_owned())
        })
        .collect();
    vec![observation(
        job,
        &endpoint.id,
        &format!("{image}@{digest}"),
        Data::Artifact {
            manifest: manifest_kind(text(value, &["/mediaType", "/imageManifestMediaType"])),
            children: vec![],
            image: image.clone(),
            digest: digest.into(),
            tags,
            revision: None,
            repository: None,
            built: false,
            created_at: timestamp(value, &["/uploadTime", "/imagePushedAt"]),
        },
    )]
}
pub fn built(job: &Job, endpoint: &Endpoint, value: &Value) -> Vec<Observation> {
    if !endpoint.id.starts_with("builds/") || text(value, &["/status"]) != Some("SUCCESS") {
        return vec![];
    }
    let revision = text(
        value,
        &[
            "/sourceProvenance/resolvedRepoSource/commitSha",
            "/substitutions/COMMIT_SHA",
        ],
    );
    let pipeline = text(value, &["/buildTriggerId"]).unwrap_or("");
    let repository = job
        .target
        .build_repositories
        .get(pipeline)
        .cloned()
        .or_else(|| {
            text(value, &["/substitutions/REPO_FULL_NAME"])
                .filter(|repo| {
                    repo.split('/').count() == 2 && monitor_core::config::validate::identifier(repo)
                })
                .map(String::from)
        });
    let created_at = timestamp(value, &["/finishTime", "/createTime"]);
    value
        .pointer("/results/images")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(128)
        .filter_map(|value| {
            let reference = text(value, &["/name"])?;
            let image = images::canonical(reference)?;
            if !job.target.resources.is_empty()
                && !job
                    .target
                    .resources
                    .iter()
                    .any(|selector| image.contains(selector))
            {
                return None;
            }
            let digest = images::digest(text(value, &["/digest"])?)?;
            Some(observation(
                job,
                &endpoint.id,
                &format!("{image}@{digest}/build"),
                Data::Artifact {
                    manifest: ManifestKind::Unknown,
                    children: vec![],
                    image: image.clone(),
                    digest: digest.into(),
                    tags: images::tag(reference)
                        .map(String::from)
                        .into_iter()
                        .collect(),
                    revision: revision.map(monitor_integrations::projection::identity),
                    repository: repository.clone(),
                    built: true,
                    created_at,
                },
            ))
        })
        .collect()
}

pub fn build(job: &Job, endpoint: &Endpoint, value: &Value) -> Vec<Observation> {
    let name = text(value, &["/id", "/name", "/arn"]).unwrap_or("build");
    let obs = |data| observation(job, &endpoint.id, name, data);
    let pipeline = text(value, &["/buildTriggerId", "/projectName"]).unwrap_or("");
    let revision = text(
        value,
        &[
            "/sourceProvenance/resolvedRepoSource/commitSha",
            "/substitutions/COMMIT_SHA",
            "/substitutions/SHORT_SHA",
            "/resolvedSourceVersion",
        ],
    )
    .unwrap_or("");
    vec![obs(Data::Build {
        superseded: false,
        pipeline: monitor_integrations::projection::identity(pipeline),
        revision: monitor_integrations::projection::identity(revision),
        target: job
            .target
            .build_targets
            .get(pipeline)
            .cloned()
            .or_else(|| {
                text(
                    value,
                    &["/substitutions/_APP_NAME", "/substitutions/_SERVICE_NAME"],
                )
                .map(monitor_integrations::projection::identity)
            })
            .unwrap_or_default(),
        state: monitor_integrations::projection::state(text(value, &["/status", "/buildStatus"])),
        created_at: timestamp(value, &["/createTime", "/startTime"]),
    })]
}

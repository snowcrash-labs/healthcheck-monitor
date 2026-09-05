//! OCI index metadata links platform manifests without downloading image layers or configuration.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, images, model::*};
use monitor_integrations::projection::{observation, text};
use serde_json::{Value, json};
pub fn validate(
    job: &Job,
    endpoint: &Endpoint,
    payload: &Value,
) -> Result<(), monitor_integrations::transport::Error> {
    use monitor_integrations::transport::Error;
    let rows = if endpoint.aws.is_some() {
        payload
            .get("images")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().collect::<Vec<_>>())
            .ok_or(Error::Malformed)?
    } else {
        vec![payload]
    };
    if rows.is_empty() {
        return Err(Error::Missing);
    }
    for row in rows {
        let manifest = if let Some(raw) = text(row, &["/imageManifest"]) {
            serde_json::from_str(raw).map_err(|_| Error::Malformed)?
        } else {
            row.clone()
        };
        if manifest.get("schemaVersion").and_then(Value::as_u64) != Some(2) {
            return Err(Error::Malformed);
        }
        let children = manifest
            .get("manifests")
            .and_then(Value::as_array)
            .ok_or(Error::Malformed)?;
        if children.len() > job.settings.max_series {
            return Err(Error::Limit);
        }
        if children
            .iter()
            .any(|child| text(child, &["/digest"]).and_then(images::digest).is_none())
        {
            return Err(Error::Malformed);
        }
    }
    Ok(())
}
pub fn followups(job: &Job, parent: &Endpoint, row: &Value) -> Vec<Endpoint> {
    if crate::artifact_projection::manifest_kind(text(
        row,
        &["/mediaType", "/imageManifestMediaType"],
    )) != ManifestKind::Index
    {
        return vec![];
    }
    if job.target.provider == Provider::Gcp {
        let Some(uri) = text(row, &["/uri"]) else {
            return vec![];
        };
        let Some(image) = images::canonical(uri) else {
            return vec![];
        };
        let Some(digest) = images::digest(uri) else {
            return vec![];
        };
        let Some((host, path)) = image.split_once('/') else {
            return vec![];
        };
        if !host.ends_with("-docker.pkg.dev")
            || !path.starts_with(&format!("{}/", job.target.scope))
        {
            return vec![];
        }
        return vec![Endpoint::get(
            format!("registry-manifest/{image}@{digest}"),
            format!("https://{host}/v2/{path}/manifests/{digest}"),
            "",
        )];
    }
    if job.target.provider == Provider::Aws
        && let Some((_, region, _)) = &parent.aws
        && let (Some(repository), Some(digest)) = (
            parent
                .body
                .as_ref()
                .and_then(|body| text(body, &["/repositoryName"])),
            text(row, &["/imageDigest"]).and_then(images::digest),
        )
    {
        let mut endpoint = Endpoint::get(
            format!("registry-manifest/{region}/{repository}/{digest}"),
            parent.url.clone(),
            "/images",
        );
        endpoint.aws = Some((
            "ecr".into(),
            region.clone(),
            "AmazonEC2ContainerRegistry_V20150921.BatchGetImage".into(),
        ));
        endpoint.body =
            Some(json!({"repositoryName":repository,"imageIds":[{"imageDigest":digest}]}));
        return vec![endpoint];
    }
    vec![]
}
pub fn project(job: &Job, endpoint: &Endpoint, row: &Value) -> Vec<Observation> {
    let manifest = if let Some(raw) = text(row, &["/imageManifest"]) {
        serde_json::from_str(raw).ok()
    } else {
        Some(row.clone())
    };
    let Some(manifest) = manifest else {
        return vec![];
    };
    let (image, digest) = if job.target.provider == Provider::Gcp {
        let Some(reference) = endpoint.id.strip_prefix("registry-manifest/") else {
            return vec![];
        };
        let Some(image) = images::canonical(reference) else {
            return vec![];
        };
        let Some(digest) = images::digest(reference) else {
            return vec![];
        };
        (image, digest.to_owned())
    } else {
        let Some((_, region, _)) = &endpoint.aws else {
            return vec![];
        };
        let Some(repository) = endpoint
            .body
            .as_ref()
            .and_then(|body| text(body, &["/repositoryName"]))
        else {
            return vec![];
        };
        let Some(digest) = text(row, &["/imageId/imageDigest"]).and_then(images::digest) else {
            return vec![];
        };
        (
            format!(
                "{}.dkr.ecr.{region}.amazonaws.com/{repository}",
                job.target.scope
            ),
            digest.to_owned(),
        )
    };
    let children = manifest
        .get("manifests")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|child| text(child, &["/digest"]).and_then(images::digest))
        .take(job.settings.max_series)
        .map(String::from)
        .collect();
    vec![observation(
        job,
        &endpoint.id,
        "index",
        Data::Artifact {
            image,
            digest,
            tags: vec![],
            revision: None,
            repository: None,
            built: false,
            created_at: None,
            manifest: crate::artifact_projection::manifest_kind(text(&manifest, &["/mediaType"])),
            children,
        },
    )]
}

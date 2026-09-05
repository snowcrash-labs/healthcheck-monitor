//! ACR repository and manifest metadata includes platform references without image downloads.
use crate::common::{Endpoint, Source};
use monitor_core::{config::resolve::Job, images, model::*};
use monitor_integrations::{
    projection::{observation, operation, text, timestamp},
    transport::Error,
};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
pub async fn enrich<S: Source>(
    source: &S,
    job: &Job,
    result: &mut CheckResult,
    cancel: &CancellationToken,
) {
    let hosts: Vec<_> = result
        .observations
        .iter()
        .filter_map(|obs| match &obs.data {
            Data::Registry { host } => Some(host.clone()),
            _ => None,
        })
        .collect();
    for host in hosts {
        let catalog = Endpoint::get(
            format!("acr-catalog/{host}"),
            format!(
                "https://{host}/v2/_catalog?n={}",
                job.settings.page_size.min(100)
            ),
            "/repositories",
        );
        let (repositories, status, catalog_pages) = pages(
            source,
            job,
            catalog,
            cancel,
            job.settings
                .max_assets
                .saturating_sub(result.observations.len()),
        )
        .await;
        result.operations.push(operation(
            &format!("acr-catalog/{host}"),
            status.as_ref().copied(),
            catalog_pages,
            job.settings.required,
        ));
        for repository in repositories.iter().filter_map(Value::as_str) {
            if !monitor_core::config::validate::identifier(repository) {
                result.operations.push(operation(
                    "acr-repository",
                    Err(&Error::Malformed),
                    0,
                    true,
                ));
                continue;
            }
            if !job.target.resources.is_empty()
                && !job
                    .target
                    .resources
                    .iter()
                    .any(|selector| repository.contains(selector))
            {
                continue;
            }
            let id = format!("acr-manifests/{host}/{repository}");
            let endpoint = Endpoint::get(
                &id,
                format!(
                    "https://{host}/acr/v1/{repository}/_manifests?api-version=2021-07-01&n={}",
                    job.settings.page_size.min(100)
                ),
                "/manifests",
            );
            let (manifests, status, page_count) = pages(
                source,
                job,
                endpoint,
                cancel,
                job.settings
                    .max_assets
                    .saturating_sub(result.observations.len()),
            )
            .await;
            let mut valid = true;
            for manifest in manifests {
                match project(job, &id, &host, repository, &manifest) {
                    Some(obs) => result.observations.push(obs),
                    None => valid = false,
                }
            }
            let status = if valid { status } else { Err(Error::Malformed) };
            result.operations.push(operation(
                &id,
                status.as_ref().copied(),
                page_count,
                job.settings.required,
            ));
        }
    }
}
async fn pages<S: Source>(
    source: &S,
    job: &Job,
    mut endpoint: Endpoint,
    cancel: &CancellationToken,
    limit: usize,
) -> (Vec<Value>, Result<usize, Error>, usize) {
    let mut out = Vec::new();
    let mut count = 0;
    let mut status = Ok(0);
    let mut previous = String::new();
    let mut bytes = 0usize;
    if limit == 0 {
        return (out, Err(Error::Limit), 0);
    }
    for page in 0..job.settings.max_pages {
        count = page + 1;
        match source.request(&endpoint, job, cancel).await {
            Ok(value) => {
                let Some(rows) = value.pointer(&endpoint.items).and_then(Value::as_array) else {
                    status = Err(Error::Malformed);
                    break;
                };
                let remaining = limit.saturating_sub(out.len());
                for row in rows.iter().take(remaining) {
                    match normalize(row, job) {
                        Ok(value) => {
                            let size = serde_json::to_vec(&value)
                                .map_or(usize::MAX, |value| value.len().saturating_mul(4));
                            if bytes.saturating_add(size) > job.settings.memory_bytes / 8 {
                                status = Err(Error::Limit);
                                break;
                            }
                            bytes += size;
                            out.push(value);
                        }
                        Err(error) => {
                            status = Err(error);
                            break;
                        }
                    }
                }
                if status.is_err() {
                    break;
                }
                if rows.len() > remaining {
                    status = Err(Error::Limit);
                    break;
                }
                let next = if let Some(next) = text(&value, &["/_monitor_next", "/nextLink"]) {
                    Some(next.to_owned())
                } else if rows.len() >= job.settings.page_size.min(100) {
                    let last = rows
                        .last()
                        .and_then(|row| row.as_str().or_else(|| text(row, &["/digest"])));
                    last.and_then(|last| {
                        let mut url = url::Url::parse(&endpoint.url).ok()?;
                        let query: Vec<_> = url
                            .query_pairs()
                            .filter(|(key, _)| key != "last")
                            .map(|(key, value)| (key.into_owned(), value.into_owned()))
                            .collect();
                        url.query_pairs_mut()
                            .clear()
                            .extend_pairs(query)
                            .append_pair("last", last);
                        Some(url.to_string())
                    })
                } else {
                    None
                };
                let Some(next) = next else {
                    break;
                };
                if next == previous || count == job.settings.max_pages || out.len() == limit {
                    status = Err(Error::Limit);
                    break;
                }
                let Ok(old) = url::Url::parse(&endpoint.url) else {
                    status = Err(Error::Malformed);
                    break;
                };
                match old.join(&next) {
                    Ok(next) if next.origin() == old.origin() && next.path() == old.path() => {
                        endpoint.url = next.to_string()
                    }
                    _ => {
                        status = Err(Error::Forbidden);
                        break;
                    }
                }
                previous = next;
            }
            Err(error) => {
                status = Err(error);
                break;
            }
        }
    }
    let rows = out.len();
    (out, status.map(|_| rows), count)
}
fn normalize(value: &Value, job: &Job) -> Result<Value, Error> {
    if let Some(repository) = value.as_str() {
        return if monitor_core::config::validate::identifier(repository) {
            Ok(Value::String(repository.into()))
        } else {
            Err(Error::Malformed)
        };
    }
    let digest = text(value, &["/digest"])
        .and_then(images::digest)
        .ok_or(Error::Malformed)?;
    let refs = value.get("references").and_then(Value::as_array);
    if refs.is_some_and(|refs| refs.len() > job.settings.max_series) {
        return Err(Error::Limit);
    }
    let references: Vec<_> = refs
        .into_iter()
        .flatten()
        .map(|reference| {
            text(reference, &["/digest"])
                .and_then(images::digest)
                .map(|digest| serde_json::json!({"digest":digest}))
                .ok_or(Error::Malformed)
        })
        .collect::<Result<_, _>>()?;
    let tags = value.get("tags").and_then(Value::as_array);
    if tags.is_some_and(|tags| tags.len() > 32) {
        return Err(Error::Limit);
    }
    let tags: Vec<_> = tags
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(monitor_integrations::projection::identity)
        .collect();
    Ok(
        serde_json::json!({"digest":digest,"tags":tags,"references":references,"architecture":text(value,&["/architecture"]).map(monitor_integrations::projection::identity),"createdTime":text(value,&["/createdTime"]).filter(|value|value.len()<64),"lastUpdateTime":text(value,&["/lastUpdateTime"]).filter(|value|value.len()<64)}),
    )
}
fn project(
    job: &Job,
    id: &str,
    host: &str,
    repository: &str,
    value: &Value,
) -> Option<Observation> {
    let digest = images::digest(text(value, &["/digest"])?)?;
    let references = value.get("references").and_then(Value::as_array);
    if references.is_some_and(|refs| refs.len() > job.settings.max_series) {
        return None;
    }
    let children: Vec<_> = references
        .into_iter()
        .flatten()
        .filter_map(|reference| text(reference, &["/digest"]).and_then(images::digest))
        .map(String::from)
        .collect();
    if references.is_some_and(|refs| refs.len() != children.len()) {
        return None;
    }
    let tags = value
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .take(32)
        .map(monitor_integrations::projection::identity)
        .collect();
    let manifest = if !children.is_empty() {
        ManifestKind::Index
    } else if text(value, &["/architecture"]).is_some() {
        ManifestKind::Image
    } else {
        ManifestKind::Unknown
    };
    Some(observation(
        job,
        id,
        digest,
        Data::Artifact {
            image: format!("{host}/{repository}"),
            digest: digest.into(),
            tags,
            revision: None,
            repository: None,
            built: false,
            created_at: timestamp(value, &["/lastUpdateTime", "/createdTime"]),
            manifest,
            children,
        },
    ))
}

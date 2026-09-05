//! Bounded commit membership and desired-reference metadata without messages or diffs.
use crate::{
    projection::{observation, operation, text},
    transport::{Error, Http},
};
use monitor_core::{config::resolve::Job, model::*};
use serde_json::Value;
use std::collections::BTreeSet;
use tokio_util::sync::CancellationToken;
async fn get(
    http: &Http,
    job: &Job,
    token: &str,
    path: &str,
    cancel: &CancellationToken,
) -> Result<Value, Error> {
    let request = http
        .client()
        .get(format!("https://api.github.com/{path}"))
        .bearer_auth(token)
        .header("X-GitHub-Api-Version", "2022-11-28")
        .build()
        .map_err(|_| Error::Malformed)?;
    http.json(request, &job.settings, cancel).await
}
pub async fn collect(
    http: &Http,
    job: &Job,
    token: &str,
    cancel: &CancellationToken,
) -> CheckResult {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    result.operations.clear();
    let repositories: BTreeSet<_> = job
        .target
        .repositories
        .iter()
        .chain(job.target.change.iter().map(|change| &change.repository))
        .collect();
    for repository in repositories {
        let id = format!("commits/{repository}");
        if result.observations.len() >= job.settings.max_assets {
            result
                .operations
                .push(operation(&id, Err(&Error::Limit), 0, true));
            break;
        }
        let reference = if let Some(reference) =
            job.target.repository_refs.get(repository).or_else(|| {
                (job.target.desired_file.is_none()
                    || job.target.repositories.first() == Some(repository))
                .then_some(job.target.source_ref.as_ref())
                .flatten()
            }) {
            Ok(reference.clone())
        } else {
            get(http, job, token, &format!("repos/{repository}"), cancel)
                .await
                .and_then(|value| {
                    text(&value, &["/default_branch"])
                        .map(String::from)
                        .ok_or(Error::Missing)
                })
        };
        let reference = match reference {
            Ok(reference) if monitor_core::config::validate::identifier(&reference) => reference,
            Ok(_) => {
                result
                    .operations
                    .push(operation(&id, Err(&Error::Malformed), 0, true));
                continue;
            }
            Err(error) => {
                result.operations.push(operation(&id, Err(&error), 1, true));
                continue;
            }
        };
        let mut pages = 0;
        let head = resolve(http, job, token, repository, &reference, cancel).await;
        if let Err(error) = &head {
            result.operations.push(operation(
                &format!("reference/{repository}"),
                Err(error),
                1,
                true,
            ));
        } else if let Ok(revision) = &head {
            let operation_id = format!("reference/{repository}");
            result.observations.push(observation(
                job,
                &operation_id,
                revision,
                Data::Commit {
                    repository: repository.to_ascii_lowercase(),
                    revision: revision.clone(),
                    reference: Some(reference.clone()),
                },
            ));
            result
                .operations
                .push(operation(&operation_id, Ok(1), 1, job.settings.required));
        }
        let mut status = Ok(0usize);
        let mut seen = BTreeSet::new();
        for page in 1..=job.settings.max_pages {
            pages = page;
            let query = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("sha", &reference)
                .append_pair(
                    "since",
                    &(chrono::Utc::now()
                        - chrono::Duration::seconds(job.settings.runtime_window.0 as i64))
                    .to_rfc3339(),
                )
                .append_pair("per_page", &job.settings.page_size.min(100).to_string())
                .append_pair("page", &page.to_string())
                .finish();
            match get(
                http,
                job,
                token,
                &format!("repos/{repository}/commits?{query}"),
                cancel,
            )
            .await
            {
                Ok(value) => {
                    let Some(rows) = value.as_array() else {
                        status = Err(Error::Malformed);
                        break;
                    };
                    for row in rows {
                        let Some(revision) = text(row, &["/sha"]).filter(|revision| {
                            revision.len() == 40
                                && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
                        }) else {
                            status = Err(Error::Malformed);
                            continue;
                        };
                        if !seen.insert(revision.to_owned()) {
                            continue;
                        }
                        if result.observations.len() >= job.settings.max_assets {
                            status = Err(Error::Limit);
                            break;
                        }
                        result.observations.push(observation(
                            job,
                            &id,
                            revision,
                            Data::Commit {
                                repository: repository.to_ascii_lowercase(),
                                revision: revision.into(),
                                reference: (head.as_ref().ok().map(String::as_str)
                                    == Some(revision))
                                .then(|| reference.clone()),
                            },
                        ));
                    }
                    if status.is_err() || rows.len() < job.settings.page_size.min(100) {
                        break;
                    }
                    if page == job.settings.max_pages {
                        status = Err(Error::Limit);
                    }
                }
                Err(error) => {
                    status = Err(error);
                    break;
                }
            }
        }
        result.operations.push(operation(
            &id,
            status.map(|_| seen.len()).as_ref().copied(),
            pages,
            job.settings.required,
        ));
    }
    result.finished_at = chrono::Utc::now();
    result
}
async fn resolve(
    http: &Http,
    job: &Job,
    token: &str,
    repository: &str,
    reference: &str,
    cancel: &CancellationToken,
) -> Result<String, Error> {
    if reference.len() == 40 && reference.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        let value = get(
            http,
            job,
            token,
            &format!("repos/{repository}/commits/{reference}"),
            cancel,
        )
        .await?;
        return if text(&value, &["/sha"]) == Some(reference) {
            Ok(reference.into())
        } else {
            Err(Error::Missing)
        };
    }
    let reference = reference.strip_prefix("refs/").unwrap_or(reference);
    let reference = if reference.starts_with("heads/") || reference.starts_with("tags/") {
        reference.into()
    } else {
        format!("heads/{reference}")
    };
    let mut value = get(
        http,
        job,
        token,
        &format!("repos/{repository}/git/ref/{reference}"),
        cancel,
    )
    .await?;
    for _ in 0..8 {
        let sha = text(&value, &["/object/sha"])
            .filter(|sha| sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or(Error::Malformed)?;
        match text(&value, &["/object/type"]) {
            Some("commit") => return Ok(sha.into()),
            Some("tag") => {
                value = get(
                    http,
                    job,
                    token,
                    &format!("repos/{repository}/git/tags/{sha}"),
                    cancel,
                )
                .await?
            }
            _ => return Err(Error::Malformed),
        }
    }
    Err(Error::Limit)
}

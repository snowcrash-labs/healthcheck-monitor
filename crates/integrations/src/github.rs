//! Typed GitHub reads and structural parsing of desired Cloud Build targets.
use super::{
    projection::{observation, operation, state, text, timestamp},
    transport::{Error, Http},
};
use monitor_core::{config::resolve::Job, model::*};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
struct BuildFile {
    steps: Vec<BuildStep>,
}
#[derive(Deserialize)]
struct BuildStep {
    id: Option<String>,
}
pub fn build_targets(input: &str) -> Result<Vec<String>, Error> {
    if input.len() > 1024 * 1024 {
        return Err(Error::Limit);
    }
    let file: BuildFile = serde_yaml_ng::from_str(input).map_err(|_| Error::Malformed)?;
    let targets: BTreeSet<_> = file
        .steps
        .into_iter()
        .filter_map(|s| s.id)
        .filter_map(|id| id.strip_prefix("build-").map(super::projection::identity))
        .filter(|s| !s.is_empty())
        .collect();
    if targets.is_empty() {
        return Err(Error::Malformed);
    }
    Ok(targets.into_iter().collect())
}
pub async fn collect(
    http: &Http,
    job: &Job,
    token: &str,
    expected: Option<&str>,
    cancel: &CancellationToken,
) -> CheckResult {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    result.operations.clear();
    let mut budget = monitor_core::collection_budget::Limit::new(&job.settings);
    let mut paths = vec![("identity".to_string(), "user".to_string(), None)];
    if job.check != Check::Preflight {
        if job.target.provider == Provider::Github {
            paths.push((
                "repositories".into(),
                format!("orgs/{}/repos", job.target.scope),
                None,
            ));
        }
        for repository in &job.target.repositories {
            let repo = if repository.contains('/') {
                repository.clone()
            } else {
                format!("{}/{repository}", job.target.scope)
            };
            paths.extend([
                (
                    format!("pulls/{repository}"),
                    format!("repos/{repo}/pulls?state=open"),
                    None,
                ),
                (
                    format!("workflows/{repository}"),
                    format!("repos/{repo}/actions/runs"),
                    Some("workflow_runs"),
                ),
                (
                    format!("deployments/{repository}"),
                    format!("repos/{repo}/deployments"),
                    None,
                ),
            ]);
        }
    }
    for (id, path, array) in paths {
        let mut outcome = Ok(0usize);
        let mut pages = 0;
        for page in 1..=job.settings.max_pages {
            pages = page;
            let request = super::github_requests::list(http, job, token, &id, &path, page);
            let response = match request {
                Ok(r) => http.json(r, &job.settings, cancel).await,
                Err(_) => Err(Error::Malformed),
            };
            match response {
                Ok(payload) => {
                    if id == "identity" {
                        let login = text(&payload, &["/login"]);
                        if expected.is_some_and(|expected| {
                            !login.is_some_and(|login| login.eq_ignore_ascii_case(expected))
                        }) {
                            result
                                .operations
                                .push(operation(&id, Err(&Error::Forbidden), 1, true));
                            return result;
                        }
                        if login.is_none() {
                            outcome = Err(Error::Malformed);
                            break;
                        }
                        budget.observations(
                            &mut result.observations,
                            [observation(
                                job,
                                &id,
                                "current",
                                Data::Identity {
                                    scope: text(&payload, &["/login"])
                                        .map(super::projection::identity)
                                        .unwrap_or_default(),
                                },
                            )],
                        );
                        outcome = Ok(1);
                        break;
                    }
                    let rows = array
                        .and_then(|key| payload.get(key))
                        .unwrap_or(&payload)
                        .as_array();
                    let Some(rows) = rows else {
                        outcome = Err(Error::Malformed);
                        break;
                    };
                    for row in rows {
                        if result.observations.len() >= job.settings.max_assets {
                            outcome = Err(Error::Limit);
                            break;
                        }
                        let name = if id.starts_with("workflows/") {
                            row.get("id")
                                .and_then(Value::as_u64)
                                .map(|id| id.to_string())
                        } else {
                            text(row, &["/full_name", "/name", "/sha"])
                                .map(String::from)
                                .or_else(|| {
                                    row.get("id").and_then(Value::as_u64).map(|n| n.to_string())
                                })
                        };
                        let Some(name) = name else { continue };
                        let data = if id.starts_with("workflows/")
                            && super::github_requests::pipeline(row).is_some_and(|pipeline| {
                                job.target.build_targets.contains_key(pipeline)
                            }) {
                            Data::Build {
                                superseded: false,
                                pipeline: super::github_requests::pipeline(row)
                                    .map(super::projection::identity)
                                    .unwrap_or_default(),
                                revision: text(row, &["/head_sha"])
                                    .map(super::projection::identity)
                                    .unwrap_or_default(),
                                target: super::github_requests::pipeline(row)
                                    .and_then(|pipeline| job.target.build_targets.get(pipeline))
                                    .cloned()
                                    .unwrap_or_default(),
                                state: state(text(row, &["/conclusion", "/status"])),
                                created_at: timestamp(row, &["/created_at"]),
                            }
                        } else {
                            Data::Inventory {
                                family: id.clone(),
                                supported: true,
                            }
                        };
                        if !budget.observations(
                            &mut result.observations,
                            [observation(job, &id, &name, data)],
                        ) {
                            outcome = Err(Error::Limit);
                            break;
                        }
                    }
                    outcome = outcome.map(|n| n + rows.len());
                    if id.starts_with("workflows/")
                        && page * super::github_requests::page_size(job, &id) >= 1000
                        && payload
                            .get("total_count")
                            .and_then(Value::as_u64)
                            .is_some_and(|total| total > 1000)
                    {
                        outcome = Err(Error::Limit);
                    }
                    if outcome.is_err() || rows.len() < super::github_requests::page_size(job, &id)
                    {
                        break;
                    }
                    if page == job.settings.max_pages {
                        outcome = Err(Error::Limit);
                    }
                }
                Err(error) => {
                    outcome = Err(error);
                    break;
                }
            }
        }
        budget.operations(
            &mut result.operations,
            [operation(
                &id,
                outcome.as_ref().copied(),
                pages,
                job.settings.required,
            )],
        );
    }
    if job.check != Check::Preflight
        && let (Some(file), Some(repo)) =
            (&job.target.desired_file, job.target.repositories.first())
    {
        use base64::Engine;
        let path = format!("https://api.github.com/repos/{repo}/contents/{file}");
        let request = http
            .client()
            .get(path)
            .query(&[("ref", job.target.source_ref.as_deref().unwrap_or("main"))])
            .bearer_auth(token)
            .build();
        let outcome = async {
            let payload = http
                .json(
                    request.map_err(|_| Error::Malformed)?,
                    &job.settings,
                    cancel,
                )
                .await?;
            let encoded = text(&payload, &["/content"]).ok_or(Error::Malformed)?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded.replace('\n', ""))
                .map_err(|_| Error::Malformed)?;
            let text = std::str::from_utf8(&bytes).map_err(|_| Error::Malformed)?;
            let targets = build_targets(text)?;
            for target in &targets {
                if !budget.observations(
                    &mut result.observations,
                    [observation(
                        job,
                        "desired-targets",
                        target,
                        Data::Inventory {
                            family: "build-target".into(),
                            supported: true,
                        },
                    )],
                ) {
                    return Err(Error::Limit);
                }
            }
            Ok(targets.len())
        }
        .await;
        result.operations.push(operation(
            "desired-targets",
            outcome.as_ref().copied(),
            1,
            true,
        ));
    }
    result.finished_at = chrono::Utc::now();
    if job.check != Check::Preflight {
        let mut remaining = job.clone();
        remaining.settings.max_assets = job
            .settings
            .max_assets
            .saturating_sub(result.observations.len());
        let commits = super::github_commits::collect(http, &remaining, token, cancel).await;
        budget.merge(&mut result, commits);
    }
    if job.target.change.is_some() && job.check != Check::Preflight {
        let changed = super::changes::collect(http, job, token, cancel).await;
        budget.merge(&mut result, changed);
    }
    budget.finish(&mut result, job.settings.required);
    result.finished_at = chrono::Utc::now();
    result
}

//! Change-aware workload identities without retaining diffs or configuration values.
use super::{
    projection::{observation, operation},
    transport::{Error, Http},
};
use monitor_core::{
    config::{resolve::Job, types::ChangeScope},
    model::*,
};
use serde::Deserialize;
use std::collections::BTreeSet;
use tokio_util::sync::CancellationToken;
#[derive(Deserialize)]
struct Comparison {
    files: Option<Vec<ChangedFile>>,
}
#[derive(Deserialize)]
struct ChangedFile {
    filename: String,
    previous_filename: Option<String>,
}
#[derive(Deserialize)]
struct Manifest {
    kind: Option<String>,
    metadata: Option<Metadata>,
}
#[derive(Deserialize)]
struct Metadata {
    name: Option<String>,
    namespace: Option<String>,
}
/// Explicit path mappings handle templated charts whose rendered names cannot be inferred.
pub fn affected(scope: &ChangeScope, paths: &[String]) -> (Vec<String>, Vec<String>) {
    let mut workloads = BTreeSet::new();
    let mut unresolved = BTreeSet::new();
    for path in paths
        .iter()
        .filter(|path| scope.paths.iter().any(|prefix| path.starts_with(prefix)))
    {
        let match_name = scope
            .workloads
            .iter()
            .filter(|(prefix, _)| path.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len());
        if let Some((_, workload)) = match_name {
            workloads.insert(workload.clone());
        } else {
            unresolved.insert(path.clone());
        }
    }
    (
        workloads.into_iter().collect(),
        unresolved.into_iter().collect(),
    )
}
pub fn manifest_workload(content: &str) -> Result<Option<String>, Error> {
    if content.len() > 1024 * 1024 {
        return Err(Error::Limit);
    }
    let manifest: Manifest = serde_yaml_ng::from_str(content).map_err(|_| Error::Malformed)?;
    if !matches!(
        manifest.kind.as_deref(),
        Some("Deployment" | "StatefulSet" | "DaemonSet" | "Job" | "CronJob")
    ) {
        return Ok(None);
    }
    let Some(metadata) = manifest.metadata else {
        return Err(Error::Malformed);
    };
    let name = metadata.name.ok_or(Error::Malformed)?;
    let namespace = metadata.namespace.unwrap_or_else(|| "default".into());
    if !monitor_core::config::validate::identifier(&name)
        || !monitor_core::config::validate::identifier(&namespace)
    {
        return Err(Error::Malformed);
    }
    Ok(Some(format!("{namespace}/{name}")))
}
pub async fn collect(
    http: &Http,
    job: &Job,
    token: &str,
    cancel: &CancellationToken,
) -> CheckResult {
    use base64::Engine;
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    result.operations.clear();
    let Some(scope) = &job.target.change else {
        return result;
    };
    let request = http
        .client()
        .get(format!(
            "https://api.github.com/repos/{}/compare/{}...{}",
            scope.repository, scope.base, scope.head
        ))
        .bearer_auth(token)
        .header("X-GitHub-Api-Version", "2022-11-28")
        .build();
    let outcome = async {
        let payload = http
            .json(
                request.map_err(|_| Error::Malformed)?,
                &job.settings,
                cancel,
            )
            .await?;
        let comparison: Comparison =
            serde_json::from_value(payload).map_err(|_| Error::Malformed)?;
        let files = comparison.files.ok_or(Error::Missing)?;
        let truncated = files.len() >= 300;
        let paths: Vec<_> = files
            .into_iter()
            .flat_map(|file| std::iter::once(file.filename).chain(file.previous_filename))
            .collect();
        let (mut workloads, unresolved) = affected(scope, &paths);
        let mut incomplete = truncated;
        for path in unresolved.into_iter().take(job.settings.max_series) {
            if !monitor_core::config::validate::identifier(&path)
                || (!path.ends_with(".yaml") && !path.ends_with(".yml"))
            {
                incomplete = true;
                continue;
            }
            let request = http
                .client()
                .get(format!(
                    "https://api.github.com/repos/{}/contents/{path}",
                    scope.repository
                ))
                .query(&[("ref", &scope.head)])
                .bearer_auth(token)
                .build();
            let discovered = async {
                let value = http
                    .json(
                        request.map_err(|_| Error::Malformed)?,
                        &job.settings,
                        cancel,
                    )
                    .await?;
                let content = value
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or(Error::Malformed)?;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(content.replace('\n', ""))
                    .map_err(|_| Error::Malformed)?;
                manifest_workload(std::str::from_utf8(&bytes).map_err(|_| Error::Malformed)?)
            }
            .await;
            match discovered {
                Ok(Some(workload)) => workloads.push(workload),
                _ => incomplete = true,
            }
        }
        workloads.sort();
        workloads.dedup();
        for workload in &workloads {
            result.observations.push(observation(
                job,
                "deployment-changes",
                workload,
                Data::Inventory {
                    family: "affected-workload".into(),
                    supported: true,
                },
            ));
        }
        if incomplete {
            Err(Error::Limit)
        } else {
            Ok(workloads.len())
        }
    }
    .await;
    result.operations.push(operation(
        "deployment-changes",
        outcome.as_ref().copied(),
        1,
        true,
    ));
    result.finished_at = chrono::Utc::now();
    result
}

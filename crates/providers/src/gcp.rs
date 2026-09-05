//! GCP native credentials and bounded metadata adapters, including regional builds.
use crate::{
    auth::Auth,
    common::{self, Endpoint},
};
use chrono::{Duration, Utc};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{
    logs::Groups,
    projection::{observation, operation, text},
    transport::{Error, Http},
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

pub fn endpoints(job: &Job) -> Vec<Endpoint> {
    let project = &job.target.scope;
    if job.check == Check::Preflight {
        return vec![Endpoint::get(
            "project-scope",
            format!("https://cloudresourcemanager.googleapis.com/v3/projects/{project}"),
            "",
        )];
    }
    if job.check == Check::Alerts {
        return vec![
            Endpoint::get(
                "open-alerts",
                format!(
                    "https://monitoring.googleapis.com/v3/projects/{project}/alerts?filter=state%3DOPEN"
                ),
                "/alerts",
            ),
            Endpoint::get(
                "snoozes",
                format!("https://monitoring.googleapis.com/v3/projects/{project}/snoozes"),
                "/snoozes",
            ),
        ];
    }
    if job.check == Check::Slo {
        return vec![Endpoint::get(
            "slo-services",
            format!("https://monitoring.googleapis.com/v3/projects/{project}/services"),
            "/services",
        )];
    }
    let regions: Vec<_> = job
        .target
        .regions
        .iter()
        .map(String::as_str)
        .chain(std::iter::once("global"))
        .collect();
    let mut endpoints = Vec::new();
    for (id, template, items) in CATALOG {
        if job.check == Check::Releases
            && !matches!(
                *id,
                "builds" | "build-triggers" | "artifact-repositories" | "cloud-run"
            )
        {
            continue;
        }
        if job.check == Check::Managed
            && !matches!(
                *id,
                "sql"
                    | "redis"
                    | "valkey"
                    | "buckets"
                    | "kms-keyrings"
                    | "secrets"
                    | "provider-health"
                    | "quotas"
            )
        {
            continue;
        }
        for region in &regions {
            if *region == "global"
                && template.contains("{r}")
                && !matches!(*id, "builds" | "build-triggers")
            {
                continue;
            }
            if !template.contains("{r}") && *region != regions[0] {
                continue;
            }
            endpoints.push(Endpoint::get(
                format!("{id}/{region}"),
                format!(
                    "https://{}",
                    template.replace("{p}", project).replace("{r}", region)
                ),
                items,
            ));
        }
    }
    for secret in &job.target.watched_secrets {
        endpoints.push(Endpoint::get(format!("secret-versions/{secret}"), format!("https://secretmanager.googleapis.com/v1/projects/{project}/secrets/{secret}/versions"), "/versions"));
    }
    endpoints
}
pub async fn collect(
    http: &Http,
    auth: &Auth,
    job: &Job,
    cancel: &CancellationToken,
) -> CheckResult {
    if job.check == Check::Metrics || job.check == Check::Queues {
        let mut job = job.clone();
        if job.target.metrics.is_empty() {
            job.target.metrics = crate::metric_catalog::gcp();
        }
        return crate::metrics::gcp(http, auth, &job, cancel).await;
    }
    if job.check == Check::Logs {
        return logs(http, auth, job, cancel).await;
    }
    common::collect(http, auth, job, endpoints(job), cancel).await
}
async fn logs(http: &Http, auth: &Auth, job: &Job, cancel: &CancellationToken) -> CheckResult {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    result.operations.clear();
    for (id, window, limit, runtime) in [
        (
            "errors",
            job.settings.log_window,
            job.settings.log_entries,
            false,
        ),
        (
            "runtime",
            job.settings.runtime_window,
            job.settings.runtime_entries,
            true,
        ),
    ] {
        let since = Utc::now() - Duration::seconds(window.0 as i64);
        let filter = format!(
            "timestamp>=\"{}\" AND {}",
            since.to_rfc3339(),
            if runtime {
                "severity>=ERROR AND (SEARCH(\"ImportError\") OR SEARCH(\"ModuleNotFoundError\") OR SEARCH(\"panic\") OR SEARCH(\"OOMKilled\"))"
            } else {
                "severity>=ERROR"
            }
        );
        let mut endpoint = Endpoint::get(
            id,
            "https://logging.googleapis.com/v2/entries:list",
            "/entries",
        );
        endpoint.body = Some(
            json!({"resourceNames":[format!("projects/{}", job.target.scope)],"filter":filter,"orderBy":"timestamp desc","pageSize":limit.min(job.settings.page_size)}),
        );
        let mut groups = Groups::default();
        let mut count = 0;
        let mut outcome = Ok(0);
        let mut pages = 0;
        for _ in 0..job.settings.max_pages {
            pages += 1;
            match common::request(http, auth, &endpoint, job, cancel).await {
                Ok(value) => {
                    if let Some(rows) = value.get("entries").and_then(|v| v.as_array()) {
                        for row in rows {
                            if count >= limit {
                                outcome = Err(Error::Limit);
                                break;
                            }
                            if let Some(message) = text(
                                row,
                                &[
                                    "/textPayload",
                                    "/jsonPayload/message",
                                    "/jsonPayload/msg",
                                    "/protoPayload/status/message",
                                ],
                            ) {
                                let time = monitor_integrations::projection::timestamp(
                                    row,
                                    &["/timestamp"],
                                )
                                .unwrap_or(result.started_at);
                                groups.add(message, time);
                            }
                            count += 1;
                        }
                    }
                    let token = text(&value, &["/nextPageToken"]).unwrap_or("");
                    if outcome.is_err() || token.is_empty() {
                        break;
                    }
                    if let Some(body) = &mut endpoint.body {
                        body["pageToken"] = json!(token);
                    }
                    if pages == job.settings.max_pages {
                        outcome = Err(Error::Limit);
                    }
                }
                Err(e) => {
                    outcome = Err(e);
                    break;
                }
            }
        }
        for (index, data) in groups.finish().into_iter().enumerate() {
            result
                .observations
                .push(observation(job, id, &index.to_string(), data));
        }
        result.operations.push(operation(
            id,
            outcome.map(|_| count).as_ref().copied(),
            pages,
            true,
        ));
    }
    result.finished_at = Utc::now();
    result
}
use crate::gcp_catalog::CATALOG;

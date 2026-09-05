//! GCP diagnostics use bounded, closed windows and preserve incomplete-page outcomes.
use crate::common::{Endpoint, Source};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{
    log_window::Window,
    projection::{text, timestamp},
    transport::Error,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
pub async fn collect_from<S: Source>(
    source: &S,
    job: &Job,
    cancel: &CancellationToken,
) -> CheckResult {
    let mut result = crate::router::base(job);
    for (id, span, limit, runtime) in [
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
        let mut window = Window::new(job, id, span, limit, source.dedupe());
        let condition = if runtime {
            "severity>=ERROR AND (SEARCH(\"ImportError\") OR SEARCH(\"ModuleNotFoundError\") OR SEARCH(\"panic\") OR SEARCH(\"OOMKilled\"))"
        } else {
            "severity>=ERROR"
        };
        let filter = format!(
            "timestamp>=\"{}\" AND timestamp<=\"{}\" AND {condition}",
            window.start.to_rfc3339(),
            window.end.to_rfc3339()
        );
        let mut endpoint = Endpoint::get(
            id,
            "https://logging.googleapis.com/v2/entries:list",
            "/entries",
        );
        endpoint.body = Some(
            json!({"resourceNames":[format!("projects/{}",job.target.scope)],"filter":filter,"orderBy":"timestamp desc","pageSize":limit.min(job.settings.page_size)}),
        );
        let mut outcome = Ok(());
        let mut pages = 0;
        let mut previous = String::new();
        for page in 0..job.settings.max_pages {
            pages = page + 1;
            match source.request(&endpoint, job, cancel).await {
                Ok(payload) => {
                    let rows = match payload.get("entries") {
                        Some(Value::Array(rows)) => rows.as_slice(),
                        None if payload.as_object().is_some_and(|map| map.is_empty()) => &[],
                        _ => {
                            outcome = Err(Error::Malformed);
                            break;
                        }
                    };
                    for row in rows {
                        let Some(at) = timestamp(row, &["/timestamp"]) else {
                            outcome = Err(Error::Malformed);
                            continue;
                        };
                        let Some(message) = text(
                            row,
                            &[
                                "/textPayload",
                                "/jsonPayload/message",
                                "/jsonPayload/msg",
                                "/protoPayload/status/message",
                            ],
                        ) else {
                            outcome = Err(Error::Malformed);
                            continue;
                        };
                        let namespace =
                            text(row, &["/resource/labels/namespace_name"]).unwrap_or("project");
                        let workload = text(
                            row,
                            &[
                                "/labels/k8s-pod~1app",
                                "/resource/labels/container_name",
                                "/resource/labels/service_name",
                                "/resource/labels/function_name",
                            ],
                        )
                        .unwrap_or("unknown");
                        let scope = format!("{namespace}/{workload}");
                        if !job.target.resources.is_empty()
                            && !job
                                .target
                                .resources
                                .iter()
                                .any(|selector| scope.contains(selector))
                        {
                            continue;
                        }
                        if let Err(error) =
                            window.record(&scope, text(row, &["/insertId"]), message, at)
                        {
                            outcome = Err(error);
                            break;
                        }
                    }
                    let token = text(&payload, &["/nextPageToken"]).unwrap_or("");
                    if outcome.is_err() || token.is_empty() {
                        break;
                    }
                    if previous == token
                        || pages == job.settings.max_pages
                        || window.scanned >= limit
                    {
                        outcome = Err(Error::Limit);
                        break;
                    }
                    previous = token.into();
                    if let Some(body) = &mut endpoint.body {
                        body["pageToken"] = json!(token);
                    }
                }
                Err(error) => {
                    outcome = Err(error);
                    break;
                }
            }
        }
        window.finish(job, id, &mut result, outcome, pages);
    }
    result.finished_at = chrono::Utc::now();
    result
}

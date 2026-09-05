//! Bounded AWS and Azure diagnostics retain only fixed, redacted signatures.
use crate::{
    auth::Auth,
    common::{self, Endpoint},
};
use chrono::{Duration, Utc};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{
    logs::Groups,
    projection::{observation, operation, text, timestamp},
    transport::{Error, Http},
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
pub async fn aws(http: &Http, auth: &Auth, job: &Job, cancel: &CancellationToken) -> CheckResult {
    let mut result = empty(job);
    for region in &job.target.regions {
        let mut endpoint = Endpoint::get(
            format!("log-groups/{region}"),
            format!("https://logs.{region}.amazonaws.com/"),
            "/logGroups",
        );
        endpoint.aws = Some((
            "logs".into(),
            region.clone(),
            "Logs_20140328.DescribeLogGroups".into(),
        ));
        endpoint.body = Some(json!({"limit":50}));
        let mut groups = Vec::new();
        for page in 0..job.settings.max_pages {
            match common::request(http, auth, &endpoint, job, cancel).await {
                Ok(value) => {
                    if let Some(rows) = value.get("logGroups").and_then(Value::as_array) {
                        for row in rows {
                            if groups.len() >= job.settings.max_series {
                                break;
                            }
                            if let Some(name) = text(row, &["/logGroupName"]) {
                                groups.push(name.to_string());
                            }
                        }
                    }
                    let token = text(&value, &["/nextToken"]).unwrap_or("");
                    if token.is_empty() {
                        result.operations.push(operation(
                            &endpoint.id,
                            Ok(groups.len()),
                            page + 1,
                            true,
                        ));
                        break;
                    }
                    if page + 1 == job.settings.max_pages || groups.len() >= job.settings.max_series
                    {
                        result.operations.push(operation(
                            &endpoint.id,
                            Err(&Error::Limit),
                            page + 1,
                            true,
                        ));
                        break;
                    }
                    if let Some(body) = &mut endpoint.body {
                        body["nextToken"] = json!(token);
                    }
                }
                Err(error) => {
                    result
                        .operations
                        .push(operation(&endpoint.id, Err(&error), page + 1, true));
                    break;
                }
            }
        }
        for (window_id, window, limit, pattern) in [
            (
                "errors",
                job.settings.log_window,
                job.settings.log_entries,
                "?ERROR ?error ?Exception ?panic",
            ),
            (
                "runtime",
                job.settings.runtime_window,
                job.settings.runtime_entries,
                "?ImportError ?ModuleNotFoundError ?panic ?OOMKilled",
            ),
        ] {
            let mut remaining = limit;
            for group in &groups {
                let id = format!("logs/{region}/{window_id}/{group}");
                let mut request = Endpoint::get(
                    &id,
                    format!("https://logs.{region}.amazonaws.com/"),
                    "/events",
                );
                request.aws = Some((
                    "logs".into(),
                    region.clone(),
                    "Logs_20140328.FilterLogEvents".into(),
                ));
                request.body = Some(
                    json!({"logGroupName":group,"startTime":(Utc::now()-Duration::seconds(window.0 as i64)).timestamp_millis(),"endTime":Utc::now().timestamp_millis(),"filterPattern":pattern,"limit":remaining.min(job.settings.page_size)}),
                );
                if remaining == 0 {
                    result
                        .operations
                        .push(operation(&id, Err(&Error::Limit), 0, true));
                    break;
                }
                let mut aggregate = Groups::default();
                let mut count = 0;
                let mut outcome = Ok(0);
                let mut pages = 0;
                for _ in 0..job.settings.max_pages {
                    pages += 1;
                    match common::request(http, auth, &request, job, cancel).await {
                        Ok(value) => {
                            if let Some(events) = value.get("events").and_then(Value::as_array) {
                                for event in events {
                                    if remaining == 0 {
                                        outcome = Err(Error::Limit);
                                        break;
                                    }
                                    if let Some(message) = text(event, &["/message"]) {
                                        let at = event
                                            .get("timestamp")
                                            .and_then(Value::as_i64)
                                            .and_then(chrono::DateTime::from_timestamp_millis)
                                            .unwrap_or(result.started_at);
                                        aggregate.add(message, at);
                                        count += 1;
                                        remaining -= 1;
                                    }
                                }
                            }
                            let token = text(&value, &["/nextToken"]).unwrap_or("");
                            if outcome.is_err() || token.is_empty() {
                                break;
                            }
                            if pages == job.settings.max_pages {
                                outcome = Err(Error::Limit);
                                break;
                            }
                            if let Some(body) = &mut request.body {
                                body["nextToken"] = json!(token);
                            }
                        }
                        Err(error) => {
                            outcome = Err(error);
                            break;
                        }
                    }
                }
                append(job, &mut result, &id, aggregate);
                result.operations.push(operation(
                    &id,
                    outcome.map(|_| count).as_ref().copied(),
                    pages,
                    true,
                ));
            }
        }
    }
    result.finished_at = Utc::now();
    result
}
pub async fn azure(http: &Http, auth: &Auth, job: &Job, cancel: &CancellationToken) -> CheckResult {
    let mut result = empty(job);
    let endpoint = Endpoint::get(
        "log-workspaces",
        format!(
            "https://management.azure.com/subscriptions/{}/providers/Microsoft.OperationalInsights/workspaces?api-version=2023-09-01",
            job.target.scope
        ),
        "/value",
    );
    let payload = match common::request(http, auth, &endpoint, job, cancel).await {
        Ok(value) => value,
        Err(error) => {
            result
                .operations
                .push(operation("log-workspaces", Err(&error), 1, true));
            return result;
        }
    };
    let workspaces = payload.get("value").and_then(Value::as_array);
    let Some(workspaces) = workspaces else {
        result
            .operations
            .push(operation("log-workspaces", Err(&Error::Missing), 1, true));
        return result;
    };
    let token = match auth.bearer_for(true).await {
        Ok(token) => token,
        Err(error) => {
            result
                .operations
                .push(operation("log-credentials", Err(&error), 0, true));
            return result;
        }
    };
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
        for workspace in workspaces.iter().take(job.settings.max_series) {
            let Some(workspace_id) = text(workspace, &["/properties/customerId"]) else {
                continue;
            };
            if !monitor_core::config::validate::identifier(workspace_id) {
                continue;
            }
            let filter = if runtime {
                "| where Message has_any ('ImportError', 'ModuleNotFoundError', 'panic', 'OOMKilled')"
            } else {
                "| where Message has_any ('ERROR', 'error', 'Exception', 'panic')"
            };
            let query = format!(
                "union isfuzzy=true (ContainerLogV2 | project TimeGenerated, Message=tostring(LogMessage)), (AppExceptions | project TimeGenerated, Message=OuterMessage) | where TimeGenerated >= datetime({}) {filter} | project TimeGenerated, Message | take {}",
                (Utc::now() - Duration::seconds(window.0 as i64)).to_rfc3339(),
                limit + 1
            );
            let request = http
                .client()
                .post(format!(
                    "https://api.loganalytics.azure.com/v1/workspaces/{workspace_id}/query"
                ))
                .bearer_auth(&token)
                .json(&json!({"query":query,"timespan":format!("PT{}S",window.0)}))
                .build();
            let payload = match request {
                Ok(r) => http.json(r, &job.settings, cancel).await,
                Err(_) => Err(Error::Malformed),
            };
            let mut aggregate = Groups::default();
            let outcome = match payload {
                Ok(value) => {
                    let rows = value.pointer("/tables/0/rows").and_then(Value::as_array);
                    match rows {
                        Some(rows) => {
                            for row in rows.iter().take(limit) {
                                let message = row.get(1).and_then(Value::as_str).unwrap_or("");
                                let at = timestamp(row, &["/0"]).unwrap_or(result.started_at);
                                aggregate.add(message, at);
                            }
                            if rows.len() > limit {
                                Err(Error::Limit)
                            } else {
                                Ok(rows.len())
                            }
                        }
                        None => Err(Error::Malformed),
                    }
                }
                Err(error) => Err(error),
            };
            let id = format!("logs/{workspace_id}/{id}");
            append(job, &mut result, &id, aggregate);
            result
                .operations
                .push(operation(&id, outcome.as_ref().copied(), 1, true));
        }
    }
    if result.operations.is_empty() {
        result
            .operations
            .push(operation("log-workspaces", Err(&Error::Missing), 1, true));
    }
    result.finished_at = Utc::now();
    result
}
fn empty(job: &Job) -> CheckResult {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    result.operations.clear();
    result
}
fn append(job: &Job, result: &mut CheckResult, id: &str, groups: Groups) {
    for (signature, data) in groups.finish_scoped() {
        result
            .observations
            .push(observation(job, id, &signature, data));
    }
}

//! CloudWatch log-group discovery and bounded, incremental diagnostic windows.
use crate::common::{Endpoint, Source};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{
    log_window::Window,
    projection::{operation, text},
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
    for region in &job.target.regions {
        let id = format!("log-groups/{region}");
        let mut endpoint = Endpoint::get(
            &id,
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
        let mut token = String::new();
        let mut pages = 0;
        let mut outcome = Ok(());
        for page in 0..job.settings.max_pages {
            pages = page + 1;
            match source.request(&endpoint, job, cancel).await {
                Ok(value) => {
                    let Some(rows) = value.get("logGroups").and_then(Value::as_array) else {
                        outcome = Err(Error::Malformed);
                        break;
                    };
                    for row in rows {
                        let Some(name) = text(row, &["/logGroupName"]) else {
                            outcome = Err(Error::Malformed);
                            continue;
                        };
                        if !job.target.resources.is_empty()
                            && !job
                                .target
                                .resources
                                .iter()
                                .any(|selector| name.contains(selector))
                        {
                            continue;
                        }
                        if groups.len() >= job.settings.max_series {
                            outcome = Err(Error::Limit);
                            break;
                        }
                        groups.push(name.to_owned());
                    }
                    let next = text(&value, &["/nextToken"]).unwrap_or("");
                    if outcome.is_err() || next.is_empty() {
                        break;
                    }
                    if token == next || pages == job.settings.max_pages {
                        outcome = Err(Error::Limit);
                        break;
                    }
                    token = next.into();
                    if let Some(body) = &mut endpoint.body {
                        body["nextToken"] = json!(next);
                    }
                }
                Err(error) => {
                    outcome = Err(error);
                    break;
                }
            }
        }
        result.operations.push(operation(
            &id,
            outcome.map(|()| groups.len()).as_ref().copied(),
            pages,
            job.settings.required,
        ));
        for (kind, span, limit, pattern) in [
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
                let id = format!("logs/{region}/{kind}/{group}");
                let mut window = Window::new(job, &id, span, remaining, source.dedupe());
                let mut endpoint = Endpoint::get(
                    &id,
                    format!("https://logs.{region}.amazonaws.com/"),
                    "/events",
                );
                endpoint.aws = Some((
                    "logs".into(),
                    region.clone(),
                    "Logs_20140328.FilterLogEvents".into(),
                ));
                endpoint.body = Some(
                    json!({"logGroupName":group,"startTime":window.start.timestamp_millis(),"endTime":window.end.timestamp_millis(),"filterPattern":pattern,"limit":remaining.min(job.settings.page_size).max(1)}),
                );
                let mut pages = 0;
                let mut outcome = if remaining == 0 {
                    Err(Error::Limit)
                } else {
                    Ok(())
                };
                let mut token = String::new();
                for page in 0..job.settings.max_pages {
                    if outcome.is_err() {
                        break;
                    }
                    pages = page + 1;
                    match source.request(&endpoint, job, cancel).await {
                        Ok(value) => {
                            let Some(events) = value.get("events").and_then(Value::as_array) else {
                                outcome = Err(Error::Malformed);
                                break;
                            };
                            for event in events {
                                let Some(message) = text(event, &["/message"]) else {
                                    outcome = Err(Error::Malformed);
                                    continue;
                                };
                                let Some(at) = event
                                    .get("timestamp")
                                    .and_then(Value::as_i64)
                                    .and_then(chrono::DateTime::from_timestamp_millis)
                                else {
                                    outcome = Err(Error::Malformed);
                                    continue;
                                };
                                if let Err(error) =
                                    window.record(group, text(event, &["/eventId"]), message, at)
                                {
                                    outcome = Err(error);
                                    break;
                                }
                            }
                            let next = text(&value, &["/nextToken"]).unwrap_or("");
                            if outcome.is_err() || next.is_empty() {
                                break;
                            }
                            if token == next
                                || pages == job.settings.max_pages
                                || window.scanned >= remaining
                            {
                                outcome = Err(Error::Limit);
                                break;
                            }
                            token = next.into();
                            if let Some(body) = &mut endpoint.body {
                                body["nextToken"] = json!(next);
                            }
                        }
                        Err(error) => {
                            outcome = Err(error);
                            break;
                        }
                    }
                }
                remaining = remaining.saturating_sub(window.scanned);
                window.finish(job, &id, &mut result, outcome, pages);
            }
        }
    }
    if !result
        .observations
        .iter()
        .any(|obs| matches!(obs.data, Data::LogWindow { .. }))
    {
        result
            .operations
            .push(operation("log-groups", Err(&Error::Missing), 0, true));
    }
    result.finished_at = chrono::Utc::now();
    result
}

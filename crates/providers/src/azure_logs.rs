//! Log Analytics diagnostics query only allowlisted columns within the selected subscription.
use crate::common::{Endpoint, Source};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{
    log_window::Window,
    projection::{operation, timestamp},
    transport::Error,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
pub async fn collect_from<S: Source>(
    source: &S,
    job: &Job,
    cancel: &CancellationToken,
) -> CheckResult {
    let endpoint = Endpoint::get(
        "log-workspaces",
        format!(
            "https://management.azure.com/subscriptions/{}/providers/Microsoft.OperationalInsights/workspaces?api-version=2023-09-01",
            job.target.scope
        ),
        "/value",
    );
    let mut result = crate::common::collect_from(source, job, vec![endpoint], cancel).await;
    let workspaces: Vec<_> = result
        .observations
        .iter()
        .filter_map(|observation| match &observation.data {
            Data::LogWorkspace { workspace_id, .. } => Some(workspace_id.clone()),
            _ => None,
        })
        .take(job.settings.max_series)
        .collect();
    for (kind, span, limit, runtime) in [
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
        let mut remaining = limit;
        for workspace in &workspaces {
            let id = format!("logs/{workspace}/{kind}");
            let mut window = Window::new(job, &id, span, remaining, source.dedupe());
            let mut endpoint = Endpoint::get(
                &id,
                format!("https://api.loganalytics.azure.com/v1/workspaces/{workspace}/query"),
                "/tables/0/rows",
            );
            let filter = if runtime {
                "Message has_any ('ImportError','ModuleNotFoundError','panic','OOMKilled')"
            } else {
                "Message has_any ('ERROR','error','Exception','panic')"
            };
            let selectors = if job.target.resources.is_empty() {
                String::new()
            } else {
                format!(
                    " | where {}",
                    job.target
                        .resources
                        .iter()
                        .map(|selector| format!("Scope contains '{selector}'"))
                        .collect::<Vec<_>>()
                        .join(" or ")
                )
            };
            let query = format!(
                "union isfuzzy=true (ContainerLogV2 | project TimeGenerated, Scope=strcat(PodNamespace,'/',ContainerName), Message=tostring(LogMessage), ResourceId=_ResourceId, EventId=tostring(column_ifexists('_ItemId',''))), (AppExceptions | project TimeGenerated, Scope=AppRoleName, Message=OuterMessage, ResourceId=_ResourceId, EventId=tostring(column_ifexists('_ItemId',''))) | where ResourceId startswith '/subscriptions/{}/' | where TimeGenerated >= datetime({}) and TimeGenerated <= datetime({}) | where {filter}{selectors} | project TimeGenerated, Scope, Message, EventId | take {}",
                job.target.scope,
                window.start.to_rfc3339(),
                window.end.to_rfc3339(),
                remaining + 1
            );
            endpoint.body = Some(
                json!({"query":query,"timespan":format!("{}/{}",window.start.to_rfc3339(),window.end.to_rfc3339())}),
            );
            let outcome = if remaining == 0 {
                Err(Error::Limit)
            } else {
                match source.request(&endpoint, job, cancel).await {
                    Ok(value) => project_rows(&value, &mut window, remaining),
                    Err(error) => Err(error),
                }
            };
            remaining = remaining.saturating_sub(window.scanned);
            window.finish(job, &id, &mut result, outcome, 1);
        }
    }
    if workspaces.is_empty() {
        result.operations.push(operation(
            "diagnostic-windows",
            Err(&Error::Missing),
            0,
            true,
        ));
    }
    result.finished_at = chrono::Utc::now();
    result
}
fn project_rows(value: &Value, window: &mut Window<'_>, limit: usize) -> Result<(), Error> {
    let rows = value
        .pointer("/tables/0/rows")
        .and_then(Value::as_array)
        .ok_or(Error::Malformed)?;
    let columns = value
        .pointer("/tables/0/columns")
        .and_then(Value::as_array)
        .ok_or(Error::Malformed)?;
    for (index, name) in ["TimeGenerated", "Scope", "Message", "EventId"]
        .iter()
        .enumerate()
    {
        if columns
            .get(index)
            .and_then(|column| column.get("name"))
            .and_then(Value::as_str)
            != Some(name)
        {
            return Err(Error::Malformed);
        }
    }
    for row in rows.iter().take(limit) {
        let at = timestamp(row, &["/0"]).ok_or(Error::Malformed)?;
        let scope = row.get(1).and_then(Value::as_str).ok_or(Error::Malformed)?;
        let message = row.get(2).and_then(Value::as_str).ok_or(Error::Malformed)?;
        let id = row
            .get(3)
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty());
        window.record(scope, id, message, at)?;
    }
    if rows.len() > limit {
        Err(Error::Limit)
    } else if value.get("error").is_some() || value.get("partialError").is_some() {
        Err(Error::Unavailable)
    } else {
        Ok(())
    }
}

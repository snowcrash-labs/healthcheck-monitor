//! Provider diagnostic contracts share window accounting and preserve unrelated results.
use chrono::{Duration, Utc};
use monitor_core::{
    config::{
        resolve::{Job, Selection},
        types::Config,
    },
    model::*,
};
use monitor_integrations::transport::Error;
use monitor_providers::{
    aws_logs, azure_logs,
    common::{Endpoint, Source},
    gcp_logs,
};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex};
use tokio_util::sync::CancellationToken;
struct Fake {
    responses: Mutex<VecDeque<Result<Value, Error>>>,
    requests: Mutex<Vec<Endpoint>>,
}
impl Source for Fake {
    async fn request(
        &self,
        endpoint: &Endpoint,
        _: &Job,
        _: &CancellationToken,
    ) -> Result<Value, Error> {
        self.requests
            .lock()
            .map_err(|_| Error::Unavailable)?
            .push(endpoint.clone());
        self.responses
            .lock()
            .map_err(|_| Error::Unavailable)?
            .pop_front()
            .ok_or(Error::Missing)?
    }
}
fn source(rows: Vec<Result<Value, Error>>) -> Fake {
    Fake {
        responses: Mutex::new(rows.into()),
        requests: Mutex::new(Vec::new()),
    }
}
fn job(provider: &str) -> Result<Job, Box<dyn std::error::Error>> {
    let mut job=Config::parse(&format!("version=1\n[[targets]]\nname='test'\nprovider='{provider}'\nscope='project'\nregions=['us-east-1']"))?.resolve(&Selection::default())?.jobs.into_iter().find(|job|job.check==Check::Logs).ok_or("missing job")?;
    job.log_end = Some(Utc::now());
    Ok(job)
}
#[tokio::test]
async fn gcp_error_failure_does_not_skip_the_separate_runtime_window()
-> Result<(), Box<dyn std::error::Error>> {
    let job = job("gcp")?;
    let at = job.log_end.ok_or("time")? - Duration::seconds(1);
    let source = source(vec![
        Err(Error::Denied),
        Ok(
            json!({"entries":[{"insertId":"one","timestamp":at.to_rfc3339(),"textPayload":"ImportError private-customer-value","resource":{"labels":{"namespace_name":"ns","container_name":"worker"}}}]}),
        ),
    ]);
    let result = gcp_logs::collect_from(&source, &job, &CancellationToken::new()).await;
    assert_eq!(result.operations.len(), 2);
    assert_eq!(result.operations[0].coverage, Coverage::Denied);
    assert!(result.observations.iter().any(|obs| matches!(
        obs.data,
        Data::Log {
            signature: LogClass::Import,
            count: 1,
            ..
        }
    )));
    assert!(!serde_json::to_string(&result)?.contains("private-customer-value"));
    let requests = source.requests.lock().map_err(|_| "lock")?;
    assert!(
        requests[0]
            .body
            .as_ref()
            .and_then(|body| body.get("filter"))
            .and_then(Value::as_str)
            .is_some_and(|filter| filter.contains("timestamp<="))
    );
    Ok(())
}
#[tokio::test]
async fn cloudwatch_pagination_deduplicates_event_ids_and_reports_scanned_counts()
-> Result<(), Box<dyn std::error::Error>> {
    let job = job("aws")?;
    let at = job.log_end.ok_or("time")? - Duration::seconds(1);
    let event = json!({"eventId":"same","timestamp":at.timestamp_millis(),"message":"panic private-payload"});
    let source = source(vec![
        Ok(json!({"logGroups":[{"logGroupName":"worker"}]})),
        Ok(json!({"events":[event.clone()],"nextToken":"page2"})),
        Ok(json!({"events":[event]})),
        Ok(json!({"events":[]})),
    ]);
    let result = aws_logs::collect_from(&source, &job, &CancellationToken::new()).await;
    assert!(result.complete());
    assert!(result.observations.iter().any(|obs| matches!(
        obs.data,
        Data::LogWindow {
            scanned: 2,
            duplicates: 1,
            complete: true,
            ..
        }
    )));
    assert!(
        result
            .observations
            .iter()
            .any(|obs| matches!(obs.data, Data::Log { count: 1, .. }))
    );
    assert!(!serde_json::to_string(&result)?.contains("private-payload"));
    Ok(())
}
#[tokio::test]
async fn azure_partial_results_keep_diagnostics_and_mark_the_window_incomplete()
-> Result<(), Box<dyn std::error::Error>> {
    let job = job("azure")?;
    let at = job.log_end.ok_or("time")? - Duration::seconds(1);
    let columns =
        json!([{"name":"TimeGenerated"},{"name":"Scope"},{"name":"Message"},{"name":"EventId"}]);
    let source = source(vec![
        Ok(
            json!({"value":[{"id":"/subscriptions/project/resourceGroups/test/providers/Microsoft.OperationalInsights/workspaces/logs","properties":{"customerId":"00000000-0000-0000-0000-000000000001"}}]}),
        ),
        Ok(
            json!({"error":{"code":"PartialError"},"tables":[{"columns":columns.clone(),"rows":[[at.to_rfc3339(),"ns/worker","panic private-payload","id"]]}]}),
        ),
        Ok(json!({"tables":[{"columns":columns,"rows":[]}]})),
    ]);
    let result = azure_logs::collect_from(&source, &job, &CancellationToken::new()).await;
    assert!(!result.complete());
    assert!(result.observations.iter().any(|obs| matches!(
        obs.data,
        Data::Log {
            count: 1,
            sampled: true,
            ..
        }
    )));
    assert!(!serde_json::to_string(&result)?.contains("private-payload"));
    let requests = source.requests.lock().map_err(|_| "lock")?;
    assert!(
        requests[1]
            .body
            .as_ref()
            .and_then(|body| body.get("query"))
            .and_then(Value::as_str)
            .is_some_and(|query| query.contains("ResourceId startswith '/subscriptions/project/'"))
    );
    Ok(())
}
#[tokio::test]
async fn malformed_log_rows_cannot_be_reported_as_empty_healthy_windows()
-> Result<(), Box<dyn std::error::Error>> {
    let source = source(vec![
        Ok(json!({"entries":"broken"})),
        Ok(json!({"entries":[{"textPayload":"panic","timestamp":"not-a-time"}]})),
    ]);
    let result = gcp_logs::collect_from(&source, &job("gcp")?, &CancellationToken::new()).await;
    assert!(
        result
            .operations
            .iter()
            .all(|operation| operation.coverage == Coverage::Malformed)
    );
    assert!(result.observations.iter().all(|obs| matches!(
        obs.data,
        Data::LogWindow {
            complete: false,
            ..
        }
    )));
    Ok(())
}

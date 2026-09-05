//! Bounded pipeline execution metadata distinguishes deployment blockers from outages.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::projection::{identity, observation, text, timestamp};
use serde_json::{Value, json};
pub fn followups(job: &Job, parent: &Endpoint, value: &Value) -> Vec<Endpoint> {
    let Some((_, region, _)) = &parent.aws else {
        return vec![];
    };
    let Some(name) = text(value, &["/pipelineName"])
        .filter(|name| monitor_core::config::validate::identifier(name))
    else {
        return vec![];
    };
    let mut endpoint = Endpoint::get(
        format!("pipeline-executions/{region}/{name}"),
        format!("https://codepipeline.{region}.amazonaws.com/"),
        "/pipelineExecutionSummaries",
    );
    endpoint.aws = Some((
        "codepipeline".into(),
        region.clone(),
        "CodePipeline_20150709.ListPipelineExecutions".into(),
    ));
    endpoint.body = Some(json!({"pipelineName":name,"maxResults":job.settings.page_size.min(100)}));
    vec![endpoint]
}
pub fn project(job: &Job, endpoint: &Endpoint, value: &Value) -> Vec<Observation> {
    let pipeline = endpoint
        .body
        .as_ref()
        .and_then(|body| text(body, &["/pipelineName"]))
        .unwrap_or("");
    let name = text(value, &["/pipelineExecutionId"]).unwrap_or("execution");
    let revisions: std::collections::BTreeSet<_> = value
        .get("sourceRevisions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|source| text(source, &["/revisionId"]))
        .filter(|revision| {
            (7..=64).contains(&revision.len())
                && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        .take(128)
        .collect();
    let revision = if revisions.len() == 1 {
        revisions.first().copied().unwrap_or("")
    } else {
        ""
    };
    let status = text(value, &["/status"]);
    let state = match status {
        Some("Succeeded") => ServiceState::Ready,
        Some("Failed") => ServiceState::Failed,
        Some("InProgress" | "Stopping") => ServiceState::Starting,
        Some("Stopped" | "Cancelled" | "Superseded") => ServiceState::Stopped,
        _ => ServiceState::Unknown,
    };
    vec![observation(
        job,
        &endpoint.id,
        name,
        Data::Build {
            pipeline: identity(pipeline),
            revision: identity(revision),
            target: job
                .target
                .build_targets
                .get(pipeline)
                .cloned()
                .unwrap_or_default(),
            state,
            created_at: timestamp(value, &["/startTime"]),
            superseded: status == Some("Superseded"),
        },
    )]
}

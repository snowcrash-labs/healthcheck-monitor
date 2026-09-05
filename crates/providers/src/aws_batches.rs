//! Batch resource details without allowing batch membership to change evidence identities.
use crate::{
    common::{Endpoint, Source},
    endpoint_scan::Fetched,
};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{
    projection::{operation, text},
    transport::Error,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
pub fn followups(parent: &Endpoint, rows: &[&Value]) -> Option<Vec<Endpoint>> {
    let family = parent.id.split('/').next()?;
    let (_, region, _) = parent.aws.as_ref()?;
    let (name, action, field, limit, items) = match family {
        "codebuild" => (
            "build-details",
            "CodeBuild_20161006.BatchGetBuilds",
            "ids",
            100,
            "/builds",
        ),
        "ecs-services" => (
            "ecs-services-detail",
            "AmazonEC2ContainerServiceV20141113.DescribeServices",
            "services",
            10,
            "/services",
        ),
        "ecs-tasks" => (
            "ecs-tasks-detail",
            "AmazonEC2ContainerServiceV20141113.DescribeTasks",
            "tasks",
            100,
            "/tasks",
        ),
        _ => return None,
    };
    let ids: Vec<_> = rows.iter().filter_map(|row| row.as_str()).collect();
    Some(
        ids.chunks(limit)
            .map(|ids| {
                let hash = crate::metric_window::id(&json!(ids));
                let mut endpoint = Endpoint::get(
                    format!("batch-{name}/{region}/{hash}"),
                    parent.url.clone(),
                    items,
                );
                let mut body = json!({field:ids});
                if let Some(cluster) = parent.body.as_ref().and_then(|body| body.get("cluster")) {
                    body["cluster"] = cluster.clone();
                }
                endpoint.aws = Some((
                    parent
                        .aws
                        .as_ref()
                        .map(|(service, _, _)| service.clone())
                        .unwrap_or_default(),
                    region.clone(),
                    action.into(),
                ));
                endpoint.body = Some(body);
                endpoint
            })
            .collect(),
    )
}
pub async fn fetch<S: Source>(
    source: &S,
    job: &Job,
    endpoint: Endpoint,
    cancel: &CancellationToken,
) -> Fetched {
    let mut result = crate::router::base(job);
    let mut followups = Vec::new();
    let field = match endpoint.id.split('/').next() {
        Some("batch-build-details") => "ids",
        Some("batch-ecs-services-detail") => "services",
        _ => "tasks",
    };
    let requested: Vec<_> = endpoint
        .body
        .as_ref()
        .and_then(|body| body.get(field))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let response = source.request(&endpoint, job, cancel).await;
    let scope = endpoint
        .id
        .trim_start_matches("batch-")
        .split('/')
        .take(2)
        .collect::<Vec<_>>()
        .join("/");
    for id in requested {
        let mut item = endpoint.clone();
        item.id = format!("{scope}/{id}");
        let outcome = match &response {
            Err(error) => Err(error.coverage()),
            Ok(value) => {
                let row = value
                    .pointer(&endpoint.items)
                    .and_then(Value::as_array)
                    .and_then(|rows| {
                        rows.iter()
                            .find(|row| text(row, &["/id", "/serviceArn", "/taskArn"]) == Some(id))
                    });
                match row {
                    Some(row) => {
                        let projected = crate::resource_projection::project(job, &item, row);
                        let count = projected.len();
                        let available = job
                            .settings
                            .max_assets
                            .saturating_sub(result.observations.len());
                        result
                            .observations
                            .extend(projected.into_iter().take(available));
                        for next in crate::details::followups(job, &item, row) {
                            if followups.len() < job.settings.ready_queue {
                                followups.push(next);
                            }
                        }
                        if count > available {
                            Err(Coverage::Truncated)
                        } else {
                            Ok(count)
                        }
                    }
                    None if value
                        .pointer(&endpoint.items)
                        .is_none_or(|rows| !rows.is_array()) =>
                    {
                        Err(Coverage::Malformed)
                    }
                    None => Err(Coverage::Missing),
                }
            }
        };
        let mut op = operation(
            &item.id,
            outcome.as_ref().copied().map_err(|_| &Error::Missing),
            1,
            job.settings.required,
        );
        if let Err(coverage) = outcome {
            op.coverage = coverage;
        }
        result.operations.push(op);
    }
    if result.operations.is_empty() {
        result
            .operations
            .push(operation(&scope, Err(&Error::Malformed), 1, true));
    }
    result.finished_at = chrono::Utc::now();
    Fetched { result, followups }
}

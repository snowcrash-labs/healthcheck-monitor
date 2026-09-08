//! Bounded current-state pages never clone or transmit an entire snapshot.
use crate::{
    api::App,
    pages::{self, Direction, Page},
    response::{ApiError, json},
    view::{FindingView, Resource},
};
use axum::{
    extract::{Query, State},
    response::Response,
};
use monitor_core::model::{Health, Severity};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    target: Option<String>,
    check: Option<monitor_core::model::Check>,
    resource: Option<String>,
    q: Option<String>,
    rule: Option<String>,
    severity: Option<Severity>,
    health: Option<Health>,
    #[serde(skip_serializing)]
    cursor: Option<String>,
    #[serde(default, skip_serializing)]
    direction: Direction,
    #[serde(skip_serializing)]
    limit: Option<usize>,
}
fn bounds(filter: &Filter) -> Result<usize, ApiError> {
    let limit = filter.limit.unwrap_or(50);
    if limit == 0
        || limit > 100
        || filter.q.as_ref().is_some_and(|value| value.len() > 128)
        || filter
            .target
            .as_ref()
            .is_some_and(|value| value.len() > 128)
    {
        return Err(ApiError::BadQuery);
    }
    Ok(limit)
}
pub async fn resources(
    State(app): State<Arc<App>>,
    Query(filter): Query<Filter>,
) -> Result<Response, ApiError> {
    let limit = bounds(&filter)?;
    let view = app.bus.current().ok_or(ApiError::Waiting)?;
    let now = chrono::Utc::now();
    let query = filter.q.as_deref().unwrap_or_default().to_lowercase();
    let rows: Vec<_> = view
        .resources
        .iter()
        .filter(|resource| {
            filter
                .target
                .as_ref()
                .is_none_or(|target| &resource.target == target)
                && filter
                    .check
                    .is_none_or(|check| resource.checks.contains(&check))
                && resource.search.contains(&query)
                && filter.health.is_none_or(|health| {
                    crate::view::current_health(resource.health, resource.expires_at, now) == health
                })
        })
        .collect();
    let page = pages::select_scoped(
        rows,
        filter.cursor.as_deref(),
        &filter.direction,
        limit,
        &("resources", &filter),
    )?;
    let items = page
        .items
        .into_iter()
        .cloned()
        .map(|mut row| {
            row.health = crate::view::current_health(row.health, row.expires_at, now);
            row
        })
        .collect();
    let items = crate::resource_rows::with_findings(items, &view.findings, now);
    json(
        &Page::<crate::resource_rows::Row> {
            generation: view.generation,
            items,
            next_cursor: page.next,
            previous_cursor: page.previous,
            total: page.total,
        },
        app.response_bytes,
    )
}
pub async fn findings(
    State(app): State<Arc<App>>,
    Query(filter): Query<Filter>,
) -> Result<Response, ApiError> {
    let limit = bounds(&filter)?;
    let view = app.bus.current().ok_or(ApiError::Waiting)?;
    let now = chrono::Utc::now();
    let query = filter.q.as_deref().unwrap_or_default().to_lowercase();
    let rows: Vec<_> = view
        .findings
        .iter()
        .filter(|finding| {
            filter
                .target
                .as_ref()
                .is_none_or(|target| &finding.target == target)
                && filter
                    .check
                    .is_none_or(|check| finding.check == Some(check))
                && filter
                    .resource
                    .as_ref()
                    .is_none_or(|resource| &finding.resource == resource)
                && filter
                    .severity
                    .is_none_or(|severity| finding.severity == severity)
                && filter
                    .rule
                    .as_ref()
                    .is_none_or(|rule| rule == &finding.rule)
                && (query.is_empty()
                    || finding.resource.to_lowercase().contains(&query)
                    || finding.rule.to_lowercase().contains(&query))
        })
        .collect();
    let page = pages::select_scoped(
        rows,
        filter.cursor.as_deref(),
        &filter.direction,
        limit,
        &("findings", &filter),
    )?;
    let items = page
        .items
        .into_iter()
        .cloned()
        .map(|mut row| {
            row.stale |= row.valid_until.is_some_and(|at| now > at);
            row
        })
        .collect();
    json(
        &Page::<FindingView> {
            generation: view.generation,
            items,
            next_cursor: page.next,
            previous_cursor: page.previous,
            total: page.total,
        },
        app.response_bytes,
    )
}
#[derive(Deserialize)]
pub struct Identity {
    id: String,
    include_findings: Option<bool>,
}
#[derive(Serialize)]
struct Detail {
    generation: u64,
    resource: Resource,
    findings: Vec<FindingView>,
}
pub async fn resource(
    State(app): State<Arc<App>>,
    Query(identity): Query<Identity>,
) -> Result<Response, ApiError> {
    if identity.id.len() > 4096 {
        return Err(ApiError::BadQuery);
    }
    let view = app.bus.current().ok_or(ApiError::Waiting)?;
    let index = view
        .resources
        .binary_search_by(|resource| resource.id.cmp(&identity.id))
        .map_err(|_| ApiError::NotFound)?;
    let mut resource = view
        .resources
        .get(index)
        .cloned()
        .ok_or(ApiError::NotFound)?;
    resource.health =
        crate::view::current_health(resource.health, resource.expires_at, chrono::Utc::now());
    let findings = view
        .findings
        .iter()
        .filter(|finding| {
            identity.include_findings != Some(false) && finding.resource == identity.id
        })
        .cloned()
        .collect();
    json(
        &Detail {
            generation: view.generation,
            resource,
            findings,
        },
        app.response_bytes,
    )
}

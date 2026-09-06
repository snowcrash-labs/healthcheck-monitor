//! Bounded current-state pages never clone or transmit an entire snapshot.
use crate::{
    api::App,
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
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    target: Option<String>,
    q: Option<String>,
    severity: Option<Severity>,
    health: Option<Health>,
    cursor: Option<usize>,
    limit: Option<usize>,
}
#[derive(Serialize)]
struct Page<T> {
    generation: u64,
    items: Vec<T>,
    next_cursor: Option<String>,
    total: usize,
}
fn bounds(filter: &Filter) -> Result<(usize, usize), ApiError> {
    let limit = filter.limit.unwrap_or(50);
    let cursor = filter.cursor.unwrap_or(0);
    if limit == 0
        || limit > 100
        || cursor > 1_000_000
        || filter.q.as_ref().is_some_and(|value| value.len() > 128)
        || filter
            .target
            .as_ref()
            .is_some_and(|value| value.len() > 128)
    {
        return Err(ApiError::BadQuery);
    }
    Ok((cursor, limit))
}
pub async fn resources(
    State(app): State<Arc<App>>,
    Query(filter): Query<Filter>,
) -> Result<Response, ApiError> {
    let (cursor, limit) = bounds(&filter)?;
    let view = app.bus.current().ok_or(ApiError::Waiting)?;
    let now = chrono::Utc::now();
    let query = filter.q.unwrap_or_default().to_ascii_lowercase();
    let rows: Vec<_> = view
        .resources
        .iter()
        .filter(|resource| {
            filter
                .target
                .as_ref()
                .is_none_or(|target| &resource.target == target)
                && resource.search.contains(&query)
                && filter.health.is_none_or(|health| {
                    crate::view::current_health(resource.health, resource.expires_at, now) == health
                })
        })
        .collect();
    let total = rows.len();
    let items = rows
        .into_iter()
        .skip(cursor)
        .take(limit)
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
            next_cursor: (cursor + limit < total).then(|| (cursor + limit).to_string()),
            total,
        },
        app.response_bytes,
    )
}
pub async fn findings(
    State(app): State<Arc<App>>,
    Query(filter): Query<Filter>,
) -> Result<Response, ApiError> {
    let (cursor, limit) = bounds(&filter)?;
    let view = app.bus.current().ok_or(ApiError::Waiting)?;
    let now = chrono::Utc::now();
    let query = filter.q.unwrap_or_default().to_ascii_lowercase();
    let mut rows: Vec<_> = view
        .findings
        .iter()
        .filter(|finding| {
            filter
                .target
                .as_ref()
                .is_none_or(|target| &finding.target == target)
                && filter
                    .severity
                    .is_none_or(|severity| finding.severity == severity)
                && (query.is_empty()
                    || finding.resource.to_ascii_lowercase().contains(&query)
                    || finding.rule.contains(&query))
        })
        .collect();
    rows.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.resource.cmp(&b.resource))
            .then_with(|| a.rule.cmp(&b.rule))
    });
    let total = rows.len();
    let items = rows
        .into_iter()
        .skip(cursor)
        .take(limit)
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
            next_cursor: (cursor + limit < total).then(|| (cursor + limit).to_string()),
            total,
        },
        app.response_bytes,
    )
}
#[derive(Deserialize)]
pub struct Identity {
    id: String,
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
        .filter(|finding| finding.resource == identity.id)
        .take(128)
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

//! Protected query endpoints share typed filters, pagination, and explicit persistence boundaries.
use crate::{
    api::App,
    response::{ApiError, json},
};
use axum::{
    extract::{Query, State},
    response::Response,
};
use monitor_query::{
    enums::{Category, FindingState, Severity},
    filter::{Filter, Window},
    record::Record,
    response::{Availability, Page, ScopeInfo, Summary},
};
use std::sync::Arc;

pub async fn availability(app: &App, window: Window) -> Availability {
    let mut availability = match app.history.query_availability(window).await {
        Ok(value) => value,
        Err(_) => Availability {
            requested: window,
            available_since: None,
            persisted_through: None,
            history_available: false,
            complete: false,
            gaps: vec![
                "Diagnostic history is unavailable; current observations may still be available"
                    .into(),
            ],
        },
    };
    let journal = app.bus.journal.health();
    if !journal.available || journal.queued_batches > 0 {
        availability.complete = false;
        availability
            .gaps
            .push("History publication is unavailable or behind current collection".into());
    }
    if !app.bus.running.load(std::sync::atomic::Ordering::Acquire) {
        availability.complete = false;
        availability
            .gaps
            .push("Continuous collection is stopped".into());
    }
    availability
}
pub async fn page(
    app: &App,
    mut filter: Filter,
    endpoint: &str,
    category: Option<Category>,
) -> Result<Page<Record>, ApiError> {
    let (window, before) = crate::query_cursor::resolve(&mut filter, endpoint)?;
    let availability = availability(app, window).await;
    let mut items = app
        .history
        .query_page(&filter, window, category, before)
        .await?;
    let limit = usize::from(filter.limit.unwrap_or(50));
    let more = items.len() > limit;
    items.truncate(limit);
    let next_cursor = if more {
        items
            .last()
            .map(|r| crate::query_cursor::next(&filter, endpoint, window, r))
            .transpose()?
    } else {
        None
    };
    Ok(Page {
        items,
        next_cursor,
        availability,
    })
}
pub async fn findings(
    State(app): State<Arc<App>>,
    Query(filter): Query<Filter>,
) -> Result<Response, ApiError> {
    json(
        &page(&app, filter, "findings", Some(Category::Finding)).await?,
        app.response_bytes,
    )
}
pub async fn diagnostics(
    State(app): State<Arc<App>>,
    Query(filter): Query<Filter>,
) -> Result<Response, ApiError> {
    json(
        &page(&app, filter, "diagnostics", Some(Category::Diagnostic)).await?,
        app.response_bytes,
    )
}
pub async fn checks(
    State(app): State<Arc<App>>,
    Query(filter): Query<Filter>,
) -> Result<Response, ApiError> {
    json(
        &page(&app, filter, "checks", Some(Category::Check)).await?,
        app.response_bytes,
    )
}
pub async fn resource(
    State(app): State<Arc<App>>,
    Query(filter): Query<Filter>,
) -> Result<Response, ApiError> {
    if filter.resource.is_none() {
        return Err(ApiError::BadQuery);
    }
    json(
        &page(&app, filter, "resource", None).await?,
        app.response_bytes,
    )
}
pub async fn summary(
    State(app): State<Arc<App>>,
    Query(mut filter): Query<Filter>,
) -> Result<Response, ApiError> {
    if filter.cursor.is_some() {
        return Err(ApiError::BadQuery);
    }
    let (window, _) = crate::query_cursor::resolve(&mut filter, "summary")?;
    let availability = availability(&app, window).await;
    let findings = app
        .history
        .query_count(&filter, window, Category::Finding, None)
        .await? as u64;
    let errors = app
        .history
        .query_count(&filter, window, Category::Finding, Some(Severity::Error))
        .await? as u64;
    let warnings = app
        .history
        .query_count(&filter, window, Category::Finding, Some(Severity::Warning))
        .await? as u64;
    let failed_checks = app.history.query_failed_checks(&filter, window).await? as u64;
    filter.state = Some(FindingState::Recovered);
    let recovered = app
        .history
        .query_count(&filter, window, Category::Finding, None)
        .await? as u64;
    json(
        &Summary {
            availability,
            findings,
            errors,
            warnings,
            recovered,
            failed_checks,
        },
        app.response_bytes,
    )
}
pub async fn scopes(
    State(app): State<Arc<App>>,
    Query(mut filter): Query<Filter>,
) -> Result<Response, ApiError> {
    let (window, _) = crate::query_cursor::resolve(&mut filter, "scopes")?;
    let view = app.bus.current().ok_or(ApiError::Waiting)?;
    let items: Vec<_> = view
        .targets
        .iter()
        .filter_map(|t| {
            let scope = crate::query_projection::scope(t);
            monitor_query::matching::scope(&filter, &scope, &Default::default()).then(|| {
                ScopeInfo {
                    checks: view
                        .checks
                        .iter()
                        .filter(|c| c.target == t.name)
                        .map(|c| crate::query_projection::check(c.check))
                        .collect(),
                    scope,
                    current: true,
                }
            })
        })
        .collect();
    json(
        &Page {
            items,
            next_cursor: None,
            availability: availability(&app, window).await,
        },
        app.response_bytes,
    )
}

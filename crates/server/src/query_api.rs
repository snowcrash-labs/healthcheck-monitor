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
    let availability = crate::query_availability::scoped(app, &filter, window).await;
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
    Query(mut filter): Query<Filter>,
) -> Result<Response, ApiError> {
    filter.normalize().map_err(|_| ApiError::BadQuery)?;
    let id = filter.resource.as_ref().ok_or(ApiError::BadQuery)?;
    let view = app.bus.current();
    let current = view.as_ref().and_then(|view| {
        let resource = view.resources.iter().find(|r| &r.id == id)?;
        let target = view.targets.iter().find(|t| t.name == resource.target)?;
        let mut scope = crate::query_projection::scope(target);
        if let Some(c) = &resource.context {
            scope.provider = crate::query_projection::provider(c.provider);
            scope.scope = c.scope.clone();
        }
        let location = crate::query_projection::location(resource.context.as_ref());
        if !monitor_query::matching::scope(&filter, &scope, &location) {
            return None;
        }
        Some(monitor_query::response::Resource {
            id: resource.id.clone(),
            scope,
            location,
            observed_at: resource.observed_at,
            valid_until: resource.expires_at,
            health: crate::query_projection::health(crate::view::current_health(
                resource.health,
                resource.expires_at,
                chrono::Utc::now(),
            )),
            facts: resource
                .facts
                .iter()
                .map(|f| monitor_query::record::Fact {
                    label: f.label.clone(),
                    value: f.value.clone(),
                })
                .collect(),
            links: resource
                .links
                .iter()
                .map(|l| monitor_query::record::Link {
                    label: l.label.clone(),
                    url: l.url.clone(),
                })
                .collect(),
        })
    });
    let history = match page(&app, filter.clone(), "resource", None).await {
        Ok(page) => page,
        Err(ApiError::Unavailable) if current.is_some() => {
            let (window, _) = crate::query_cursor::resolve(&mut filter, "resource")?;
            Page {
                items: vec![],
                next_cursor: None,
                availability: availability(&app, window).await,
            }
        }
        Err(error) => return Err(error),
    };
    json(
        &monitor_query::response::ResourceDetail { current, history },
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
    let availability = crate::query_availability::scoped(&app, &filter, window).await;
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
    let (window, after) = crate::query_cursor::resolve_scope(&mut filter)?;
    let availability = availability(&app, window).await;
    let mut items = std::collections::BTreeMap::new();
    let key = |s: &monitor_query::record::Scope| (s.target.clone(), s.provider, s.scope.clone());
    match app
        .history
        .query_scopes(&filter, window, after.as_ref())
        .await
    {
        Ok(scopes) => {
            for scope in scopes {
                items.insert(
                    key(&scope),
                    ScopeInfo {
                        scope,
                        checks: vec![],
                        current: false,
                    },
                );
            }
        }
        Err(_) if !availability.history_available => {}
        Err(error) => return Err(error.into()),
    }
    if let Some(view) = app.bus.current() {
        for target in &view.targets {
            let scope = crate::query_projection::scope(target);
            if monitor_query::matching::scope(&filter, &scope, &Default::default())
                && after.as_ref().is_none_or(|a| key(&scope) > key(a))
            {
                items.insert(
                    key(&scope),
                    ScopeInfo {
                        checks: view
                            .checks
                            .iter()
                            .filter(|c| c.target == target.name)
                            .map(|c| crate::query_projection::check(c.check))
                            .collect(),
                        scope,
                        current: true,
                    },
                );
            }
        }
    }
    let limit = usize::from(filter.limit.unwrap_or(50));
    let more = items.len() > limit;
    let items: Vec<_> = items.into_values().take(limit).collect();
    let next_cursor = if more {
        items
            .last()
            .map(|r| crate::query_cursor::next_scope(&filter, window, r.scope.clone()))
            .transpose()?
    } else {
        None
    };
    json(
        &Page {
            items,
            next_cursor,
            availability,
        },
        app.response_bytes,
    )
}

//! Scope discovery coalesces reads, caches bounded responses, and degrades explicitly to current metadata.
use crate::{
    api::App,
    response::{ApiError, json},
    scope_cache::{Cached, Key},
};
use axum::{
    body::to_bytes,
    extract::{Query, State},
    response::Response,
};
use monitor_query::{
    filter::{Filter, Window},
    record::Scope,
    response::{Availability, Page, ScopeInfo},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

/// Collection and journal transitions must invalidate cached completeness claims immediately.
pub(super) fn cache_key(app: &App, filter: &Filter) -> Result<Key, ApiError> {
    let journal = app.bus.journal.health();
    Ok(Key {
        generation: app.bus.current().map_or(0, |view| view.generation),
        filter: serde_json::to_string(&(
            filter,
            app.history.ready(),
            app.bus.running.load(Ordering::Acquire),
            journal.available,
            journal.queued_batches > 0,
            journal.last_persisted_at,
            journal.dropped_events,
            journal.dropped_runs,
            journal.gaps,
        ))
        .map_err(|_| ApiError::BadQuery)?,
    })
}

/// Serve a validated scope page under the router's authentication guard.
pub async fn scopes(
    State(app): State<Arc<App>>,
    Query(mut filter): Query<Filter>,
) -> Result<Response, ApiError> {
    let (window, after) = crate::query_cursor::resolve_scope(&mut filter)?;
    let key = cache_key(&app, &filter)?;
    if let Some(value) = app.scope_cache.get(&key).await {
        return value.response(true);
    }
    let flight = app.scope_cache.flight(&key).await?;
    let _guard = tokio::time::timeout(Duration::from_secs(3), flight.lock())
        .await
        .map_err(|_| ApiError::Busy)?;
    if let Some(value) = app.scope_cache.get(&key).await {
        return value.response(true);
    }
    let page = load(&app, filter, window, after).await?;
    let historical = page.availability.history_available;
    let response = json(&page, app.response_bytes)?;
    let body = to_bytes(response.into_body(), app.response_bytes)
        .await
        .map_err(|_| ApiError::Capacity)?;
    let value = if historical {
        Cached::new(body)
    } else {
        Cached::current_only(body)
    };
    app.scope_cache.insert(key, value.clone()).await;
    value.response(false)
}

/// A successful watermark lookup does not prove the subsequent scope lookup succeeded.
pub(super) fn history_failed(mut availability: Availability, reason: &str) -> Availability {
    availability.history_available = false;
    availability.complete = false;
    availability.gaps.push(reason.into());
    availability
}

async fn load(
    app: &Arc<App>,
    filter: Filter,
    window: Window,
    after: Option<Scope>,
) -> Result<Page<ScopeInfo>, ApiError> {
    assemble(
        app,
        &filter,
        window,
        after.as_ref(),
        crate::query_api::availability(app, window),
        app.history.query_scopes(&filter, window, after.as_ref()),
    )
    .await
}

/// Keep historical failure handling testable without weakening the real database contract.
pub(super) async fn assemble(
    app: &App,
    filter: &Filter,
    window: Window,
    after: Option<&Scope>,
    availability: impl std::future::Future<Output = Availability>,
    scopes: impl std::future::Future<Output = Result<Vec<Scope>, monitor_history::error::Error>>,
) -> Result<Page<ScopeInfo>, ApiError> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let history = match tokio::time::timeout_at(deadline, availability).await {
        Ok(availability) => {
            let scopes = match tokio::time::timeout_at(deadline, scopes).await {
                Ok(result) => result,
                Err(_) => {
                    tracing::warn!(
                        reason = "scope_deadline",
                        "Historical scope discovery unavailable; serving current metadata"
                    );
                    return project(
                        app,
                        filter,
                        window,
                        after,
                        history_failed(
                            availability,
                            "Historical scope lookup timed out; retained scopes may be missing",
                        ),
                        vec![],
                    );
                }
            };
            Ok((availability, scopes))
        }
        Err(error) => Err(error),
    };
    let (availability, retained) = match history {
        Ok((availability, Ok(scopes))) => (availability, scopes),
        Ok((availability, Err(_))) => {
            tracing::warn!(
                reason = "lookup_failed",
                "Historical scope discovery unavailable; serving current metadata"
            );
            (
                history_failed(
                    availability,
                    "Historical scope lookup failed; retained scopes may be missing",
                ),
                vec![],
            )
        }
        Err(_) => {
            tracing::warn!(
                reason = "deadline",
                "Historical scope discovery unavailable; serving current metadata"
            );
            (
                Availability {
                    requested: window,
                    available_since: None,
                    persisted_through: None,
                    history_available: false,
                    complete: false,
                    gaps: vec![
                        "Historical scope lookup timed out; retained scopes may be missing".into(),
                    ],
                },
                vec![],
            )
        }
    };
    project(app, filter, window, after, availability, retained)
}

/// Merge retained and current scope metadata without changing the requested pagination window.
fn project(
    app: &App,
    filter: &Filter,
    window: Window,
    after: Option<&Scope>,
    availability: Availability,
    retained: Vec<Scope>,
) -> Result<Page<ScopeInfo>, ApiError> {
    let key = |s: &Scope| (s.target.clone(), s.provider, s.scope.clone());
    let mut items = BTreeMap::new();
    for scope in retained {
        items.insert(
            key(&scope),
            ScopeInfo {
                scope,
                checks: vec![],
                current: false,
            },
        );
    }
    if let Some(view) = app.bus.current() {
        for target in &view.targets {
            let scope = crate::query_projection::scope(target);
            if monitor_query::matching::scope(filter, &scope, &Default::default())
                && after.is_none_or(|after| key(&scope) > key(after))
            {
                items.insert(
                    key(&scope),
                    ScopeInfo {
                        checks: view
                            .checks
                            .iter()
                            .filter(|check| check.target == target.name)
                            .map(|check| crate::query_projection::check(check.check))
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
            .map(|row| crate::query_cursor::next_scope(filter, window, row.scope.clone()))
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

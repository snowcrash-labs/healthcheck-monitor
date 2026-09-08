//! Cache bounds, expiry, isolation, and failure coverage are observable without cloud credentials.
use crate::scope_cache::{Cache, Cached, Key};
use axum::body::Bytes;
use std::{sync::Arc, time::Duration};

fn key(filter: &str, generation: u64) -> Key {
    Key {
        generation,
        filter: filter.into(),
    }
}

#[tokio::test(start_paused = true)]
async fn scope_cache_expires_without_refreshing_age_on_hits() {
    let cache = Cache::default();
    let key = key("scope-a", 1);
    cache
        .insert(key.clone(), Cached::new(Bytes::from_static(b"one")))
        .await;
    tokio::time::advance(Duration::from_secs(10)).await;
    assert!(cache.get(&key).await.is_some());
    tokio::time::advance(Duration::from_secs(5)).await;
    assert!(cache.get(&key).await.is_none());
}

#[tokio::test(start_paused = true)]
async fn degraded_scope_pages_expire_after_one_second() {
    let cache = Cache::default();
    let key = key("degraded", 1);
    cache
        .insert(
            key.clone(),
            Cached::current_only(Bytes::from_static(b"incomplete")),
        )
        .await;
    assert!(cache.get(&key).await.is_some());
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(cache.get(&key).await.is_none());
}

#[tokio::test]
async fn scope_cache_separates_filters_cursors_and_current_view_generations() {
    let cache = Cache::default();
    cache
        .insert(
            key("target=a&cursor=first", 1),
            Cached::new(Bytes::from_static(b"one")),
        )
        .await;
    assert!(cache.get(&key("target=b&cursor=first", 1)).await.is_none());
    assert!(cache.get(&key("target=a&cursor=second", 1)).await.is_none());
    assert!(cache.get(&key("target=a&cursor=first", 2)).await.is_none());
    assert!(cache.get(&key("target=a&cursor=first", 1)).await.is_some());
}

#[tokio::test]
async fn scope_cache_evicts_least_recently_read_entries() {
    let cache = Cache::default();
    for index in 0..64 {
        cache
            .insert(
                key(&index.to_string(), 1),
                Cached::new(Bytes::from_static(b"x")),
            )
            .await;
    }
    assert!(cache.get(&key("0", 1)).await.is_some());
    cache
        .insert(key("64", 1), Cached::new(Bytes::from_static(b"x")))
        .await;
    assert!(cache.get(&key("1", 1)).await.is_none());
    assert!(cache.get(&key("0", 1)).await.is_some());
    assert!(cache.get(&key("64", 1)).await.is_some());
}

#[tokio::test]
async fn scope_cache_obeys_byte_budget_and_rejects_oversized_entries() {
    let cache = Cache::default();
    for index in 0..5 {
        cache
            .insert(
                key(&index.to_string(), 1),
                Cached::new(Bytes::from(vec![0; 7 * 1024 * 1024])),
            )
            .await;
    }
    assert!(cache.get(&key("0", 1)).await.is_none());
    assert!(cache.get(&key("4", 1)).await.is_some());
    cache
        .insert(
            key("oversized", 1),
            Cached::new(Bytes::from(vec![0; 32 * 1024 * 1024 + 1])),
        )
        .await;
    assert!(cache.get(&key("oversized", 1)).await.is_none());
    assert!(cache.get(&key("4", 1)).await.is_some());
}

#[tokio::test]
async fn identical_misses_share_a_flight_and_cancelled_flights_release_capacity()
-> Result<(), crate::response::ApiError> {
    let cache = Cache::default();
    let first = cache.flight(&key("same", 1)).await?;
    let second = cache.flight(&key("same", 1)).await?;
    assert!(Arc::ptr_eq(&first, &second));
    drop(first);
    drop(second);
    let mut flights = Vec::new();
    for index in 0..64 {
        flights.push(cache.flight(&key(&index.to_string(), 1)).await?);
    }
    assert!(cache.flight(&key("extra", 1)).await.is_err());
    drop(flights);
    assert!(cache.flight(&key("after cancellation", 1)).await.is_ok());
    Ok(())
}

#[test]
fn failed_scope_lookup_invalidates_successful_watermark_availability() {
    let now = chrono::Utc::now();
    let availability = monitor_query::response::Availability {
        requested: monitor_query::filter::Window {
            from: now - chrono::Duration::minutes(1),
            to: now,
        },
        available_since: Some(now - chrono::Duration::days(1)),
        persisted_through: Some(now),
        history_available: true,
        complete: true,
        gaps: vec![],
    };
    let degraded = crate::scope_api::history_failed(availability, "scope query failed");
    assert!(!degraded.history_available);
    assert!(!degraded.complete);
    assert_eq!(degraded.persisted_through, Some(now));
    assert_eq!(degraded.gaps, vec!["scope query failed"]);
}

#[tokio::test]
async fn cached_scope_response_keeps_its_window_and_cannot_bypass_authentication()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        api::router,
        config::Config,
        test_support::{app, request},
    };
    use axum::{body::to_bytes, http::StatusCode};
    use tower::ServiceExt;
    let config = Config::default();
    let (app, _) = app(&config).await?;
    let routes = router(app.clone(), &config);
    let response = routes
        .clone()
        .oneshot(request("/api/v1/query/scopes")?)
        .await?;
    let bytes = to_bytes(response.into_body(), 65536).await?;
    let key = crate::scope_api::cache_key(&app, &monitor_query::filter::Filter::default())
        .map_err(|_| "cache key encoding")?;
    app.scope_cache
        .insert(key, Cached::new(bytes.clone()))
        .await;
    let response = routes
        .clone()
        .oneshot(request("/api/v1/query/scopes")?)
        .await?;
    assert_eq!(response.headers()["x-healthcheck-cache"], "hit");
    assert_eq!(to_bytes(response.into_body(), 65536).await?, bytes);
    let mut hostile = request("/api/v1/query/scopes")?;
    hostile
        .headers_mut()
        .insert("host", "attacker.invalid".parse()?);
    assert_eq!(
        routes.clone().oneshot(hostile).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    use monitor_runtime::Observer;
    app.bus.heartbeat(chrono::Utc::now(), false);
    let stopped = routes
        .clone()
        .oneshot(request("/api/v1/query/scopes")?)
        .await?;
    assert_eq!(stopped.headers()["x-healthcheck-cache"], "miss");
    Ok(())
}

#[tokio::test]
async fn scope_lookup_failure_after_successful_watermark_keeps_current_scope()
-> Result<(), Box<dyn std::error::Error>> {
    let (app, _) = crate::test_support::app(&crate::config::Config::default()).await?;
    let now = chrono::Utc::now();
    let window = monitor_query::filter::Window {
        from: now - chrono::Duration::minutes(1),
        to: now,
    };
    let availability = monitor_query::response::Availability {
        requested: window,
        available_since: Some(window.from),
        persisted_through: Some(now),
        history_available: true,
        complete: true,
        gaps: vec![],
    };
    let result = crate::scope_api::assemble(&app, &Default::default(), window, None, async {
        (availability, Err(monitor_history::error::Error::Pool))
    })
    .await
    .map_err(|_| "scope fallback failed")?;
    assert_eq!(result.items.len(), 1);
    assert!(result.items[0].current);
    assert!(!result.availability.history_available);
    assert!(!result.availability.complete);
    assert!(
        result
            .availability
            .gaps
            .iter()
            .any(|gap| gap.contains("retained scopes may be missing"))
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn stalled_history_has_a_bounded_current_scope_fallback()
-> Result<(), Box<dyn std::error::Error>> {
    let (app, _) = crate::test_support::app(&crate::config::Config::default()).await?;
    let now = chrono::Utc::now();
    let window = monitor_query::filter::Window {
        from: now - chrono::Duration::minutes(1),
        to: now,
    };
    let start = tokio::time::Instant::now();
    let result = crate::scope_api::assemble(
        &app,
        &Default::default(),
        window,
        None,
        std::future::pending(),
    )
    .await
    .map_err(|_| "scope fallback failed")?;
    assert_eq!(start.elapsed(), Duration::from_secs(2));
    assert_eq!(result.items.len(), 1);
    assert!(!result.availability.complete);
    assert!(
        result
            .availability
            .gaps
            .iter()
            .any(|gap| gap.contains("timed out"))
    );
    Ok(())
}

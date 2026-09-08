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

#[tokio::test(start_paused = true)]
async fn oversized_replacement_removes_the_previous_cached_value() {
    let cache = Cache::default();
    let key = key("replacement", 1);
    cache
        .insert(key.clone(), Cached::new(Bytes::from_static(b"old")))
        .await;
    cache
        .insert(
            key.clone(),
            Cached::new(Bytes::from(vec![0; 32 * 1024 * 1024 + 1])),
        )
        .await;
    assert!(cache.get(&key).await.is_none());
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

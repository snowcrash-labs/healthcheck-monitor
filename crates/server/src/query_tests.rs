//! Query API, generated schemas, and retained diagnostic intervals use the production router.
use crate::{
    api::router,
    config::Config,
    test_support::{app, request},
};
use axum::{body::to_bytes, http::StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn query_schema_and_current_scope_remain_available_without_history()
-> Result<(), Box<dyn std::error::Error>> {
    let config = Config::default();
    let (app, _) = app(&config).await?;
    let router = router(app, &config);
    let response = router
        .clone()
        .oneshot(request("/api/v1/query/openapi.json")?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let schema: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 262144).await?)?;
    assert_eq!(schema["openapi"], "3.1.0");
    assert_eq!(schema["paths"].as_object().map(|o| o.len()), Some(7));
    assert!(schema["components"]["schemas"]["Deployment"].is_object());
    let response = router
        .oneshot(request("/api/v1/query/scopes?target=fixture")?)
        .await?;
    let page: monitor_query::response::Page<monitor_query::response::ScopeInfo> =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await?)?;
    assert_eq!(page.items.len(), 1);
    assert!(!page.availability.history_available);
    assert!(!page.availability.complete);
    Ok(())
}
#[tokio::test]
async fn current_resource_survives_database_unavailability()
-> Result<(), Box<dyn std::error::Error>> {
    let config = Config::default();
    let (app, _) = app(&config).await?;
    let response = router(app, &config)
        .oneshot(request(
            "/api/v1/query/resource?resource=fixture%2Fendpoints%2Fapi",
        )?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let detail: monitor_query::response::ResourceDetail =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await?)?;
    assert!(detail.current.is_some());
    assert!(!detail.history.availability.complete);
    Ok(())
}
#[tokio::test]
async fn queries_reject_ambiguous_scope_invalid_windows_and_writes()
-> Result<(), Box<dyn std::error::Error>> {
    let config = Config::default();
    let (app, _) = app(&config).await?;
    let router = router(app, &config);
    for path in [
        "/api/v1/query/scopes?project=a&account=b",
        "/api/v1/query/findings?limit=101",
        "/api/v1/query/findings?lookback_seconds=0",
        "/api/v1/query/resource",
    ] {
        assert_eq!(
            router.clone().oneshot(request(path)?).await?.status(),
            StatusCode::BAD_REQUEST
        );
    }
    let mut write = request("/api/v1/query/scopes")?;
    *write.method_mut() = axum::http::Method::POST;
    assert_eq!(
        router.clone().oneshot(write).await?.status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    let mut hostile = request("/api/v1/query/openapi.json")?;
    hostile
        .headers_mut()
        .insert("host", "attacker.invalid".parse()?);
    assert_eq!(
        router.oneshot(hostile).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    Ok(())
}
#[test]
fn recorder_closes_with_original_diagnostic_evidence() -> Result<(), Box<dyn std::error::Error>> {
    let (state, effective) = crate::test_support::evidence()?;
    let view = crate::build_view::build(&state.snapshot, &effective, 1);
    let finding = view.findings.first().ok_or("finding")?;
    let mut record =
        crate::query_projection::finding(finding, &view.targets).ok_or("projection")?;
    let at = record.observed_at;
    let mut recorder = crate::query_recorder::Recorder::default();
    let first = recorder.capture(record.clone(), at)?;
    assert_eq!(first.len(), 1);
    record.last_observed_at += chrono::Duration::seconds(30);
    let second = recorder.capture(record.clone(), record.last_observed_at)?;
    assert_eq!(second.len(), 1);
    assert_eq!(first[0].key, second[0].key);
    let closed = recorder
        .close(&record.identity, at + chrono::Duration::minutes(2), false)
        .ok_or("closed record")?;
    assert_eq!(closed.record.observed_at, at);
    assert_eq!(
        closed.record.state(),
        Some(monitor_query::enums::FindingState::Recovered)
    );
    assert!(
        matches!(closed.record.details,monitor_query::record::Details::Finding {facts,..} if !facts.is_empty())
    );
    Ok(())
}

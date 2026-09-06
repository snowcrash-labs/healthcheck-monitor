//! Access, pagination, redaction, and independent SSE budgets are verified through real routes.
use crate::{
    api::router,
    config::{Access, Config},
    test_support::{app, request},
};
use axum::{body::to_bytes, http::StatusCode};
use tower::ServiceExt;
#[tokio::test]
async fn search_includes_observed_facts_beyond_the_first_page()
-> Result<(), Box<dyn std::error::Error>> {
    use monitor_runtime::Observer;
    let config = Config::default();
    let (app, _) = app(&config).await?;
    let (mut state, effective) = crate::test_support::evidence()?;
    state.snapshot.findings.clear();
    let result = state.snapshot.results.values_mut().next().ok_or("result")?;
    let template = result.observations.first().ok_or("observation")?.clone();
    result.observations.clear();
    for index in 0..400 {
        let mut row = template.clone();
        row.resource = format!("fixture/resource-{index:04}");
        if index != 399 {
            row.data = monitor_core::model::Data::Identity {
                scope: "ordinary".into(),
            };
        }
        result.observations.push(row);
    }
    app.bus.update(&state.snapshot, &effective, &[]);
    let routes = router(app, &config);
    for (query, count) in [("limit=50", 400), ("q=503", 1), ("q=RESOURCE-0399", 1)] {
        let response = routes
            .clone()
            .oneshot(request(&format!("/api/v1/resources?{query}"))?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await?)?;
        assert_eq!(value["total"], count);
        if count == 1 {
            assert_eq!(value["items"][0]["id"], "fixture/resource-0399");
        }
    }
    Ok(())
}
#[tokio::test]
async fn deep_links_are_documents_and_unknown_assets_stay_missing()
-> Result<(), Box<dyn std::error::Error>> {
    let config = Config::default();
    let (app, _) = app(&config).await?;
    let routes = router(app, &config);
    for path in [
        "/findings",
        "/checks",
        "/checks/fixture/edge?target=fixture",
        "/targets/fixture?target=fixture",
        "/resources/fixture%2Fendpoints%2Fapi",
        "/history",
    ] {
        assert_eq!(
            routes.clone().oneshot(request(path)?).await?.status(),
            StatusCode::OK
        );
    }
    for path in ["/assets/missing.js", "/monitor.toml"] {
        assert_eq!(
            routes.clone().oneshot(request(path)?).await?.status(),
            StatusCode::NOT_FOUND
        );
    }
    Ok(())
}
#[tokio::test]
async fn overview_resources_findings_and_missing_history_have_separate_outcomes()
-> Result<(), Box<dyn std::error::Error>> {
    let config = Config::default();
    let (app, _) = app(&config).await?;
    let routes = router(app, &config);
    let response = routes.clone().oneshot(request("/api/v1/overview")?).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key("content-security-policy"));
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await?)?;
    assert_eq!(value["totals"]["error_findings"], 1);
    assert_eq!(value["targets"][0]["health"], "unhealthy");
    assert_eq!(value["history"]["available"], false);
    let response = routes
        .clone()
        .oneshot(request("/api/v1/resources?limit=1")?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await?)?;
    assert_eq!(value["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(value["items"][0]["finding_count"], 1);
    assert_eq!(value["items"][0]["findings"][0]["severity"], "error");
    for (path, status) in [
        ("/api/v1/resources?limit=101", StatusCode::BAD_REQUEST),
        ("/api/v1/resource?id=absent", StatusCode::NOT_FOUND),
        ("/api/v1/history", StatusCode::SERVICE_UNAVAILABLE),
        ("/api/v1/unknown", StatusCode::NOT_FOUND),
    ] {
        assert_eq!(
            routes.clone().oneshot(request(path)?).await?.status(),
            status
        );
    }
    Ok(())
}
#[tokio::test]
async fn local_access_rejects_dns_rebinding_cross_site_and_writes()
-> Result<(), Box<dyn std::error::Error>> {
    let config = Config::default();
    let (app, _) = app(&config).await?;
    let routes = router(app, &config);
    let mut rebound = request("/api/v1/overview")?;
    rebound
        .headers_mut()
        .insert("host", "attacker.example".parse()?);
    assert_eq!(
        routes.clone().oneshot(rebound).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    let mut cross = request("/api/v1/overview")?;
    cross
        .headers_mut()
        .insert("sec-fetch-site", "cross-site".parse()?);
    assert_eq!(
        routes.clone().oneshot(cross).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    let mut write = request("/api/v1/overview")?;
    *write.method_mut() = axum::http::Method::POST;
    assert_eq!(
        routes.oneshot(write).await?.status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    Ok(())
}
#[tokio::test]
async fn proxy_access_requires_secret_trusted_peer_and_allowed_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let config = Config {
        access: Access::Proxy {
            secret_env: "PROXY_KEY".into(),
            public_origin: "https://dashboard.example".parse()?,
            trusted_peers: vec!["127.0.0.1/32".parse()?],
            allowed_domains: vec!["example.com".into()],
        },
        ..Default::default()
    };
    let (app, _) = app(&config).await?;
    let routes = router(app, &config);
    for (token, email, expected) in [
        ("s".repeat(32), "reader@example.com", StatusCode::OK),
        (
            "wrong".into(),
            "reader@example.com",
            StatusCode::UNAUTHORIZED,
        ),
        (
            "s".repeat(32),
            "reader@outside.com",
            StatusCode::UNAUTHORIZED,
        ),
    ] {
        let mut request = request("/api/v1/overview")?;
        request
            .headers_mut()
            .insert("host", "dashboard.example".parse()?);
        request
            .headers_mut()
            .insert("x-healthcheck-proxy-key", token.parse()?);
        request
            .headers_mut()
            .insert("x-auth-request-email", email.parse()?);
        assert_eq!(routes.clone().oneshot(request).await?.status(), expected);
    }
    Ok(())
}
#[tokio::test]
async fn slow_bodies_keep_permits_while_sse_has_its_own_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let config = Config {
        requests: 1,
        event_streams: 1,
        ..Default::default()
    };
    let (app, _) = app(&config).await?;
    let routes = router(app, &config);
    let held = routes.clone().oneshot(request("/api/v1/overview")?).await?;
    assert_eq!(held.status(), StatusCode::OK);
    assert_eq!(
        routes
            .clone()
            .oneshot(request("/api/v1/overview")?)
            .await?
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(held);
    let events = routes.clone().oneshot(request("/api/v1/events")?).await?;
    assert_eq!(events.status(), StatusCode::OK);
    assert_eq!(
        routes
            .clone()
            .oneshot(request("/api/v1/events")?)
            .await?
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        routes.oneshot(request("/api/v1/overview")?).await?.status(),
        StatusCode::OK
    );
    drop(events);
    Ok(())
}
#[tokio::test]
async fn json_response_limits_fail_explicitly() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config {
        response_bytes: 128,
        ..Default::default()
    };
    let (app, _) = app(&config).await?;
    assert_eq!(
        router(app, &config)
            .oneshot(request("/api/v1/overview")?)
            .await?
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    Ok(())
}

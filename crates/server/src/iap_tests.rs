//! Signed claims, scope and network boundaries fail closed without live authentication calls.
use super::*;
use base64::Engine;
use jsonwebtoken::{EncodingKey, Header, encode};
const AUDIENCE: &str = "/projects/123/global/backendServices/456";

async fn fixture()
-> Result<(Iap, EncodingKey, Header, serde_json::Value), Box<dyn std::error::Error>> {
    let pair = rcgen::KeyPair::generate()?;
    let public = pair.public_key_raw();
    let x = public.get(1..33).ok_or("P-256 x")?;
    let y = public.get(33..65).ok_or("P-256 y")?;
    let key = DecodingKey::from_ec_components(
        &base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(x),
        &base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(y),
    )?;
    let verifier = Iap::new(AUDIENCE.into(), vec!["soundpatrol.com".into()])?;
    {
        let mut keys = verifier.keys.lock().await;
        keys.values.insert("fixture".into(), Arc::new(key));
        keys.valid_until = Instant::now() + Duration::from_secs(3600);
        keys.refresh_after = Instant::now() + Duration::from_secs(60);
    }
    let mut header = Header::new(Algorithm::ES256);
    header.kid = Some("fixture".into());
    let now = chrono::Utc::now().timestamp();
    let claims = serde_json::json!({"iss":"https://cloud.google.com/iap","aud":AUDIENCE,"iat":now,"exp":now+300,"sub":"accounts.google.com:fixture","email":"reader@soundpatrol.com"});
    Ok((
        verifier,
        EncodingKey::from_ec_der(&pair.serialize_der()),
        header,
        claims,
    ))
}

#[tokio::test]
async fn signatures_audience_issuer_expiry_and_domain_are_all_required()
-> Result<(), Box<dyn std::error::Error>> {
    let (verifier, key, header, claims) = fixture().await?;
    let token = encode(&header, &claims, &key)?;
    assert!(verifier.authorized(&token).await);
    let now = chrono::Utc::now().timestamp();
    for (field, value) in [
        (
            "aud",
            serde_json::json!("/projects/123/global/backendServices/789"),
        ),
        ("iss", serde_json::json!("https://accounts.google.com")),
        ("exp", serde_json::json!(now - 60)),
        ("iat", serde_json::json!(now + 120)),
        ("email", serde_json::json!("reader@outside.example")),
        ("exp", serde_json::json!(now + 7200)),
    ] {
        let mut altered = claims.clone();
        altered[field] = value;
        assert!(!verifier.authorized(&encode(&header, &altered, &key)?).await);
    }
    let mut unknown = header.clone();
    unknown.kid = Some("unknown".into());
    assert!(!verifier.authorized(&encode(&unknown, &claims, &key)?).await);
    assert!(!verifier.authorized("unsigned-token").await);
    assert!(!verifier.authorized(&"x".repeat(16385)).await);
    let forged = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(b"fixture"),
    )?;
    assert!(!verifier.authorized(&forged).await);
    let mut tampered = token.into_bytes();
    if let Some(last) = tampered.last_mut() {
        *last = if *last == b'A' { b'B' } else { b'A' };
    }
    assert!(!verifier.authorized(std::str::from_utf8(&tampered)?).await);
    Ok(())
}

#[tokio::test]
async fn stale_keys_cannot_authorize_requests() -> Result<(), Box<dyn std::error::Error>> {
    let (verifier, key, header, claims) = fixture().await?;
    verifier.keys.lock().await.valid_until = Instant::now();
    assert!(!verifier.authorized(&encode(&header, &claims, &key)?).await);
    Ok(())
}

#[tokio::test]
async fn anonymous_assets_spoofed_headers_and_direct_requests_are_blocked()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        api::router,
        config::{Access, Config},
        test_support::{app, request},
    };
    use axum::{extract::ConnectInfo, http::StatusCode};
    use tower::ServiceExt;
    let config = Config {
        access: Access::Iap {
            audience: AUDIENCE.into(),
            public_origin: "https://health.soundpatrol.com".parse()?,
            allowed_domains: vec!["soundpatrol.com".into()],
        },
        ..Default::default()
    };
    let (app, _) = app(&config).await?;
    let routes = router(app, &config);
    for path in [
        "/",
        "/resources",
        "/assets/app.js",
        "/api/v1/overview",
        "/api/v1/events",
        "/api/v1/query/scopes",
        "/api/v1/query/summary",
        "/api/v1/query/findings",
        "/api/v1/query/diagnostics",
        "/api/v1/query/resource",
        "/api/v1/query/checks",
        "/api/v1/query/deployment",
        "/api/v1/query/openapi.json",
    ] {
        let mut request = request(path)?;
        request
            .headers_mut()
            .insert("host", "health.soundpatrol.com".parse()?);
        request.headers_mut().insert(
            "x-goog-authenticated-user-email",
            "accounts.google.com:reader@soundpatrol.com".parse()?,
        );
        request
            .headers_mut()
            .insert("x-auth-request-email", "reader@soundpatrol.com".parse()?);
        request
            .extensions_mut()
            .insert(ConnectInfo(crate::listener::Peer(
                "35.191.1.1:1000".parse()?,
            )));
        assert_eq!(
            routes.clone().oneshot(request).await?.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let mut probe = request("/healthz")?;
    probe
        .extensions_mut()
        .insert(ConnectInfo(crate::listener::Peer(
            "203.0.113.1:1000".parse()?,
        )));
    assert_eq!(
        routes.clone().oneshot(probe).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        routes.oneshot(request("/healthz")?).await?.status(),
        StatusCode::OK
    );
    Ok(())
}

//! TLS ALPN negotiates HTTP/2 while stalled handshakes leave independent clients runnable.
use crate::{
    api::router,
    config::{Config, Tls},
    listener::{Listener, Peer},
    test_support::app,
};
use std::{io::Write, time::Duration};
#[tokio::test]
async fn http2_is_negotiated_without_waiting_for_a_stalled_handshake()
-> Result<(), Box<dyn std::error::Error>> {
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    let mut cert = tempfile::NamedTempFile::new()?;
    cert.write_all(certificate.cert.pem().as_bytes())?;
    let mut key = tempfile::NamedTempFile::new()?;
    key.write_all(certificate.signing_key.serialize_pem().as_bytes())?;
    let config = Config {
        listen: "127.0.0.1:0".parse()?,
        tls: Some(Tls {
            certificate: cert.path().into(),
            private_key: key.path().into(),
        }),
        ..Default::default()
    };
    let (app, _) = app(&config).await?;
    let stop = app.stop.clone();
    let listener = Listener::bind(&config).await?;
    let address = listener.address()?;
    let routes = router(app, &config);
    let shutdown = stop.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            routes.into_make_service_with_connect_info::<Peer>(),
        )
        .with_graceful_shutdown(shutdown.cancelled_owned())
        .await
    });
    let stalled = tokio::net::TcpStream::connect(address).await?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .add_root_certificate(reqwest::Certificate::from_der(certificate.cert.der())?)
        .resolve("localhost", address)
        .build()?;
    let response = tokio::time::timeout(
        Duration::from_secs(2),
        client
            .get(format!(
                "https://localhost:{}/api/v1/overview",
                address.port()
            ))
            .send(),
    )
    .await??;
    assert_eq!(response.version(), reqwest::Version::HTTP_2);
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let _: serde_json::Value = response.json().await?;
    drop(stalled);
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(2), server).await???;
    Ok(())
}

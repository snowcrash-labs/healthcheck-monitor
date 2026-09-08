//! Explicitly enabled deployment checks reuse the operator's validated Google identity.
use crate::{
    client::{Client, decode},
    config::Config,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Billing {
    enabled: bool,
    series: Vec<serde_json::Value>,
    sources: Vec<Source>,
}
#[derive(Deserialize)]
struct Source {
    id: String,
    state: String,
    fault: Option<String>,
    imported_at: Option<String>,
}

#[tokio::test]
#[ignore = "requires HEALTHCHECK_LIVE_VERIFY=1 and an authenticated local connector profile"]
async fn iap_reader_can_query_billing_while_anonymous_access_is_denied()
-> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("HEALTHCHECK_LIVE_VERIFY").as_deref() != Ok("1") {
        return Err("live verification was not explicitly enabled".into());
    }
    let _ = tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .try_init();
    let client = Client::new(Config::load(None).await?)?;
    let url = client.config.server.join("api/v1/query/costs/series")?;
    let anonymous = client.http.get(url.clone()).send().await?;
    assert!(
        anonymous.status().is_redirection() || matches!(anonymous.status().as_u16(), 401 | 403)
    );
    drop(anonymous);
    let response = client
        .http
        .get(url)
        .bearer_auth(client.token().await?)
        .send()
        .await?;
    let version = response.version();
    let raw: serde_json::Value = decode(response, 2 * 1024 * 1024).await?;
    if let Ok(path) = std::env::var("HEALTHCHECK_BILLING_CAPTURE") {
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .await?;
        file.write_all(&serde_json::to_vec(&raw)?).await?;
        file.sync_all().await?;
    }
    let view: Billing = serde_json::from_value(raw)?;
    assert!(view.enabled);
    assert!(
        !view.series.is_empty(),
        "no imported billing series is available yet"
    );
    if let Ok(required) = std::env::var("HEALTHCHECK_EXPECT_COST_SOURCES") {
        if required.len() > 1024 || required.split(',').count() > 16 {
            return Err("invalid expected billing source list".into());
        }
        for id in required.split(',') {
            let source = view
                .sources
                .iter()
                .find(|source| source.id == id)
                .ok_or("expected billing source missing")?;
            assert!(
                source.imported_at.is_some() && source.fault.is_none(),
                "expected source has no successful current import: {id}"
            );
        }
    }
    for source in view.sources {
        tracing::info!(
            source = source.id,
            state = source.state,
            fault = source.fault,
            "Billing source verified through IAP"
        );
    }
    tracing::info!(points = view.series.len(), protocol = ?version, "Authenticated billing query verified");
    Ok(())
}

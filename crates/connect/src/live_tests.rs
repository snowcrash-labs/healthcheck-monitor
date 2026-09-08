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
    verify_cost_asset(&client).await?;
    Ok(())
}

/// An API success cannot prove that the deployed binary embeds the matching browser contract.
async fn verify_cost_asset(client: &Client) -> Result<(), Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};
    let Ok(asset) = std::env::var("HEALTHCHECK_DASHBOARD_COST_ASSET") else {
        return Ok(());
    };
    let name = asset
        .strip_prefix("assets/cost-schema-")
        .and_then(|s| s.strip_suffix(".js"))
        .ok_or("invalid billing asset path")?;
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        return Err("invalid billing asset path".into());
    }
    let expected = std::env::var("HEALTHCHECK_DASHBOARD_COST_ASSET_SHA256")?;
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid billing asset checksum".into());
    }
    let mut response = client
        .http
        .get(client.config.server.join(&asset)?)
        .bearer_auth(client.token().await?)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err("billing asset unavailable".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > 2 * 1024 * 1024 {
            return Err("billing asset exceeds response bound".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let actual: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(actual, expected.to_ascii_lowercase());
    tracing::info!(asset, "Deployed billing contract asset verified");
    Ok(())
}

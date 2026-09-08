//! Explicit live probe keeps CLI authorization separate from native API collection.
use google_cloud_auth::credentials::{
    AccessToken, AccessTokenCredentialsProvider, CacheableResource, CredentialsProvider,
};
use monitor_costs::{config::Config, model::Provider, query::Period};
use monitor_history::History;
use monitor_integrations::{http_pool::Pools, process::Processes};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
struct ProbeCredential(String);
impl std::fmt::Debug for ProbeCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProbeCredential")
    }
}
impl CredentialsProvider for ProbeCredential {
    async fn headers(
        &self,
        _: http::Extensions,
    ) -> Result<CacheableResource<http::HeaderMap>, google_cloud_auth::errors::CredentialsError>
    {
        Ok(CacheableResource::NotModified)
    }
    async fn universe_domain(&self) -> Option<String> {
        Some("googleapis.com".into())
    }
}
impl AccessTokenCredentialsProvider for ProbeCredential {
    async fn access_token(
        &self,
    ) -> Result<AccessToken, google_cloud_auth::errors::CredentialsError> {
        Ok(AccessToken {
            token: self.0.clone(),
        })
    }
}
#[derive(serde::Deserialize)]
struct Input {
    costs: Config,
}
#[tokio::test]
#[ignore = "requires an explicit local validation config, isolated database, and an existing gcloud sign-in"]
async fn collect_live_billing_through_native_transport() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::var("HEALTHCHECK_BILLING_LIVE_CONFIG")?;
    let input: Input = toml::from_str(&std::fs::read_to_string(path)?)?;
    input.costs.validate()?;
    let source = input
        .costs
        .sources
        .iter()
        .find(|s| s.provider == Provider::Gcp)
        .ok_or("GCP source")?;
    let stop = CancellationToken::new();
    let mut command = tokio::process::Command::new("gcloud");
    command.args(["auth", "print-access-token", "--quiet"]);
    let output = Processes::new(1)
        .run_command(command, 65536, std::time::Duration::from_secs(20), &stop)
        .await?;
    let token = String::from_utf8(output.stdout).map_err(|_| "invalid credential encoding")?;
    let reader = monitor_providers::billing_gcp::Reader::with_credentials(
        source.gcp.clone().ok_or("GCP source")?,
        Arc::new(Pools::default()),
        ProbeCredential(token.trim().into()).into(),
    )?;
    let database = std::env::var("HEALTHCHECK_COST_LIVE_DATABASE_URL")?;
    if !database.starts_with("postgresql:///healthcheck_monitor_costs_live?host=/tmp") {
        return Err("refusing non-validation database".into());
    }
    let history = History::new(database, Default::default())?;
    history.migrate().await?;
    let to = chrono::Utc::now().date_naive();
    let import = history
        .cost_begin(
            source,
            Period {
                from: to - chrono::Duration::days(2),
                to,
            },
            &input.costs,
        )
        .await?;
    let job = format!("health_cost_{}", import.id.as_ref().simple());
    let billed = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        reader.start(
            &job,
            &source.billing_scope,
            import.period,
            input.costs.query_bytes(),
            input.costs.rows(),
            &stop,
        ),
    )
    .await??;
    history.cost_settle(&import, billed).await?;
    let mut cursor = None;
    let mut count = 0;
    loop {
        let mut page = reader.page(&job, cursor.as_deref(), &stop).await?;
        if page.total > input.costs.rows() || count + page.rows.len() > input.costs.rows() {
            return Err("live result exceeds declared bound".into());
        }
        count += page.rows.len();
        for charge in &mut page.rows {
            input.costs.attribute(charge);
        }
        history.cost_stage(&import, page.rows).await?;
        if page.next.is_none() {
            if count != page.total {
                return Err("incomplete provider result".into());
            }
            break;
        }
        if page.next == cursor {
            return Err("repeated provider cursor".into());
        }
        cursor = page.next;
    }
    history
        .cost_publish(&import, count, input.costs.retention())
        .await?;
    assert!(count > 0, "configured period returned no charge aggregates");
    Ok(())
}

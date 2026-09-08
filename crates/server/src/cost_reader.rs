//! Closed provider dispatch for billing; every adapter returns projected charges only.
use monitor_costs::{config::Source, model::Charge, query::Period};
use monitor_integrations::{http_pool::Pools, transport::Error};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
pub enum Reader {
    Gcp(monitor_providers::billing_gcp::Reader),
    Aws(monitor_providers::billing_aws::Reader),
    Azure(monitor_providers::billing_azure::Reader),
}
impl Reader {
    pub async fn new(source: &Source, pools: Arc<Pools>) -> Result<Self, Error> {
        if let Some(gcp) = &source.gcp {
            return monitor_providers::billing_gcp::Reader::new(
                gcp.clone(),
                pools,
                source.credential.as_ref(),
            )
            .await
            .map(Self::Gcp);
        }
        if source.aws_query.is_some() {
            return monitor_providers::billing_aws::Reader::new(source, pools)
                .await
                .map(Self::Aws);
        }
        if source.azure_query.is_some() {
            return monitor_providers::billing_azure::Reader::new(source, pools)
                .await
                .map(Self::Azure);
        }
        Err(Error::Forbidden)
    }
    pub async fn page(
        &self,
        period: Period,
        cursor: Option<String>,
        stop: &CancellationToken,
    ) -> Result<(Vec<Charge>, Option<String>), Error> {
        match self {
            Self::Aws(reader) => reader
                .page(period, cursor, stop)
                .await
                .map(|p| (p.rows, p.next)),
            Self::Azure(reader) => reader
                .page(period, cursor.as_deref(), stop)
                .await
                .map(|p| (p.rows, p.next)),
            Self::Gcp(_) => Err(Error::Forbidden),
        }
    }
}
pub async fn query_import(
    history: &monitor_history::History,
    source: &Source,
    config: &monitor_costs::config::Config,
    reader: &Reader,
    period: Period,
    stop: &CancellationToken,
) -> Result<(), &'static str> {
    let import = history
        .cost_begin(source, period, config)
        .await
        .map_err(|_| "Billing staging unavailable")?;
    history
        .cost_attempt(&import)
        .await
        .map_err(|_| "Billing attempt could not be recorded")?;
    history
        .cost_settle(&import, 0)
        .await
        .map_err(|_| "Billing allowance could not be recorded")?;
    let mut cursor = None;
    let mut count = 0usize;
    let pages = if source.aws_query.is_some() { 4 } else { 32 };
    for _ in 0..pages {
        let (mut rows, next) = reader
            .page(import.period, cursor.clone(), stop)
            .await
            .map_err(query_fault)?;
        count += rows.len();
        if count > config.rows() {
            return Err("Billing aggregate exceeds its record bound");
        }
        for charge in &mut rows {
            config.attribute(charge);
        }
        for batch in rows.chunks(500) {
            history
                .cost_stage(&import, batch.to_vec())
                .await
                .map_err(|_| "Billing staging failed; prior totals retained")?;
        }
        if next.is_none() {
            return history
                .cost_publish(&import, count, config.retention())
                .await
                .map_err(|_| "Billing publication failed; prior totals retained");
        }
        if next == cursor {
            return Err("Billing query repeated its cursor");
        }
        cursor = next;
    }
    Err("Billing query exceeded its page allowance")
}

/// Only the closed transport error vocabulary is safe to persist and show to readers.
fn query_fault(error: Error) -> &'static str {
    match error {
        Error::Authentication => "Billing credentials unavailable or expired",
        Error::Denied => "Billing source access denied",
        Error::Throttled => "Billing provider rate limit reached; import will retry",
        Error::Unavailable => "Billing provider unavailable; import will retry",
        Error::Timeout => "Billing query deadline expired; import will retry",
        Error::Missing => "Billing provider omitted required fields",
        Error::Malformed => "Billing provider returned an invalid aggregate",
        Error::Limit => "Billing response exceeds its configured limit",
        Error::Cancelled => "Billing query cancelled; previous totals retained",
        Error::Forbidden => "Billing query rejected by the read-only policy",
    }
}

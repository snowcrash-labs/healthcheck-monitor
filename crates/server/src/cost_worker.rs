//! One resumable billing worker has separate database and remote-operation capacity.
use monitor_costs::{config::Config, query::Period};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub async fn run(history: Arc<monitor_history::History>, config: Config, stop: CancellationToken) {
    if !config.enabled {
        return;
    }
    let mut readers = std::collections::BTreeMap::new();
    let pools = Arc::new(monitor_integrations::http_pool::Pools::default());
    loop {
        for source in &config.sources {
            if stop.is_cancelled() {
                return;
            }
            let Some(gcp) = &source.gcp else {
                continue;
            };
            if !readers.contains_key(&source.id) {
                match monitor_providers::billing_gcp::Reader::new(gcp.clone(), pools.clone()).await {
                    Ok(reader) => {
                        readers.insert(source.id.clone(), reader);
                    }
                    Err(_) => {
                        tracing::warn!(provider = "gcp", "Billing authentication unavailable");
                        continue;
                    }
                }
            }
            let Some(reader) = readers.get(&source.id) else {
                continue;
            };
            let to = match chrono::Utc::now().date_naive().succ_opt() {
                Some(to) => to,
                None => return,
            };
            let previous = history.cost_last_import(&source.id).await.ok().flatten();
            let days = if previous.is_none()
                || previous.is_some_and(|at| at.date_naive() != chrono::Utc::now().date_naive())
            {
                i64::from(config.backfill())
            } else {
                i64::from(config.backfill().min(7))
            };
            let period = Period {
                from: to - chrono::Duration::days(days),
                to,
            };
            let result = tokio::time::timeout(std::time::Duration::from_secs(300), import(&history, source, &config, reader, period, &stop)).await.unwrap_or(Err("Billing import timed out; existing job retained"));
            if let Err(fault) = result {
                if history.cost_fault(&source.id, fault).await.is_err() {
                    tracing::warn!("Billing fault could not be persisted");
                }
                tracing::warn!(fault, "Billing import deferred");
            }
        }
        if history.cost_cleanup().await.is_err() {
            tracing::warn!("Billing cleanup deferred");
        }
        tokio::select! { _=stop.cancelled()=>return, _=tokio::time::sleep(std::time::Duration::from_secs(config.interval()))=>{} }
    }
}
async fn import(
    history: &monitor_history::History,
    source: &monitor_costs::config::Source,
    config: &Config,
    reader: &monitor_providers::billing_gcp::Reader,
    period: Period,
    stop: &CancellationToken,
) -> Result<(), &'static str> {
    let import = history
        .cost_begin(source, period, config)
        .await
        .map_err(|_| "Storage or daily query allowance unavailable")?;
    let job = format!("health_cost_{}", import.id.as_ref().simple());
    reader
        .start(
            &job,
            &source.billing_scope,
            import.period,
            config.query_bytes(),
            config.rows(),
            stop,
        )
        .await
        .map_err(|error| match error {
            monitor_integrations::transport::Error::Authentication => {
                "Billing authentication unavailable"
            }
            monitor_integrations::transport::Error::Denied => "Billing source access denied",
            monitor_integrations::transport::Error::Limit => {
                "Billing query exceeds its configured scan allowance"
            }
            _ => "Billing query incomplete; its existing job will be resumed",
        })?;
    let mut cursor = None;
    let mut count = 0usize;
    let mut expected = None;
    for _ in 0..(config.rows() / 500 + 2) {
        let page = reader
            .page(&job, cursor.as_deref(), stop)
            .await
            .map_err(|_| "Billing results unavailable")?;
        if page.total > config.rows() || expected.is_some_and(|n| n != page.total) {
            return Err("Billing aggregate exceeds its bound or changed");
        }
        expected = Some(page.total);
        count = count
            .checked_add(page.rows.len())
            .ok_or("Billing aggregate exceeds its bound")?;
        if count > config.rows() {
            return Err("Billing aggregate exceeds its bound");
        }
        history
            .cost_stage(&import, page.rows)
            .await
            .map_err(|_| "Billing staging failed; previous totals retained")?;
        if page.next.is_none() {
            if count != page.total {
                return Err("Billing result pages are incomplete");
            }
            history
                .cost_publish(&import, count, config.retention())
                .await
                .map_err(|_| "Billing publication failed; previous totals retained")?;
            return Ok(());
        }
        if page.next == cursor {
            return Err("Billing result cursor repeated");
        }
        cursor = page.next;
    }
    Err("Billing result pagination exceeded its bound")
}

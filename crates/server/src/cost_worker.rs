//! One resumable billing worker has separate database and remote-operation capacity.
use monitor_costs::{config::Config, query::Period};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub async fn run(
    history: Arc<monitor_history::History>,
    config: Config,
    bus: Arc<crate::bus::Bus>,
    stop: CancellationToken,
) {
    if !config.enabled {
        return;
    }
    // The collector publishes only after acquiring its evidence-store writer lock.
    let mut ready = bus.changed.subscribe();
    while bus.current().is_none() {
        tokio::select! { _=stop.cancelled()=>return, result=ready.changed()=>{ if result.is_err() { return; } } }
    }
    let mut readers = std::collections::BTreeMap::new();
    let pools = Arc::new(monitor_integrations::http_pool::Pools::default());
    loop {
        let mut backfill_progress = false;
        for source in &config.sources {
            if stop.is_cancelled() {
                return;
            }
            if source.aws_query.is_some()
                && history
                    .cost_recent_attempt(&source.id, 86400)
                    .await
                    .unwrap_or(true)
            {
                continue;
            }
            let period = match history.cost_next_period(source, &config).await {
                Ok(Some(period)) => period,
                Ok(None) => continue,
                Err(_) => {
                    tracing::warn!("Billing schedule unavailable");
                    continue;
                }
            };
            if !readers.contains_key(&source.id) {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    crate::cost_reader::Reader::new(source, pools.clone()),
                )
                .await
                {
                    Ok(Ok(reader)) => {
                        readers.insert(source.id.clone(), reader);
                    }
                    _ => {
                        tracing::warn!(
                            provider = source.provider.as_str(),
                            "Billing authentication unavailable"
                        );
                        continue;
                    }
                }
            }
            let Some(reader) = readers.get(&source.id) else {
                continue;
            };
            let collect = async {
                match reader {
                    crate::cost_reader::Reader::Gcp(reader) => {
                        import(&history, source, &config, reader, period, &stop).await
                    }
                    _ => {
                        crate::cost_reader::query_import(
                            &history, source, &config, reader, period, &stop,
                        )
                        .await
                    }
                }
            };
            let result = tokio::time::timeout(std::time::Duration::from_secs(300), collect)
                .await
                .unwrap_or(Err("Billing import timed out; existing job retained"));
            backfill_progress |= result.is_ok();
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
        tokio::select! { _=stop.cancelled()=>return, _=tokio::time::sleep(std::time::Duration::from_secs(if backfill_progress { 30 } else { config.interval() }))=>{} }
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
    let billed = reader
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
    history
        .cost_settle(&import, billed)
        .await
        .map_err(|_| "Billing allowance could not be recorded")?;
    let mut cursor = None;
    let mut count = 0usize;
    let mut expected = None;
    for _ in 0..(config.rows() / 500 + 2) {
        let mut page = reader
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
        for charge in &mut page.rows {
            config.attribute(charge);
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

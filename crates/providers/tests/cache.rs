//! Concurrent checks reuse normalized inventories under a finite cache budget.
use monitor_core::{
    config::{resolve::Selection, types::Config},
    model::Check,
};
use monitor_integrations::transport::Error;
use monitor_providers::{
    common::{Endpoint, Source, collect_from},
    inventory_cache::InventoryCache,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{sync::Semaphore, time::Duration};
use tokio_util::sync::CancellationToken;
struct Fake {
    cache: InventoryCache,
    calls: AtomicUsize,
}
impl Source for Fake {
    fn cache(&self) -> Option<&InventoryCache> {
        Some(&self.cache)
    }
    async fn request(
        &self,
        _: &Endpoint,
        _: &monitor_core::config::resolve::Job,
        _: &CancellationToken,
    ) -> Result<Value, Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(json!({"items":[{"name":"database","state":"RUNNING","password":"discard-this"}]}))
    }
}
fn job() -> Result<monitor_core::config::resolve::Job, Box<dyn std::error::Error>> {
    Config::parse("version=1\n[[targets]]\nname='test'\nprovider='gcp'\nscope='project'")?
        .resolve(&Selection::default())?
        .jobs
        .into_iter()
        .find(|job| job.check == Check::Inventory)
        .ok_or_else(|| "missing job".into())
}
#[tokio::test(start_paused = true)]
async fn overlapping_checks_share_one_inventory_read() -> Result<(), Box<dyn std::error::Error>> {
    let source = Fake {
        cache: InventoryCache::new(16, Arc::new(Semaphore::new(1024 * 1024))),
        calls: AtomicUsize::new(0),
    };
    let first = job()?;
    let mut second = first.clone();
    second.check = Check::Managed;
    let endpoint = Endpoint::get(
        "shared-inventory",
        "https://sqladmin.googleapis.com/v1/instances",
        "/items",
    );
    let cancel = CancellationToken::new();
    let (a, b) = tokio::join!(
        collect_from(&source, &first, vec![endpoint.clone()], &cancel),
        collect_from(&source, &second, vec![endpoint.clone()], &cancel)
    );
    assert!(a.complete() && b.complete());
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    assert_eq!(a.observations[0].observed_at, b.observations[0].observed_at);
    assert!(!serde_json::to_string(&a)?.contains("discard-this"));
    let _ = collect_from(&source, &first, vec![endpoint.clone()], &cancel).await;
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    tokio::time::advance(first.settings.interval.duration() + Duration::from_secs(1)).await;
    let _ = collect_from(&source, &first, vec![endpoint], &cancel).await;
    assert_eq!(source.calls.load(Ordering::SeqCst), 3);
    Ok(())
}
#[tokio::test(start_paused = true)]
async fn exhausted_cache_budget_does_not_block_collection() -> Result<(), Box<dyn std::error::Error>>
{
    let source = Fake {
        cache: InventoryCache::new(16, Arc::new(Semaphore::new(0))),
        calls: AtomicUsize::new(0),
    };
    let job = job()?;
    let cancel = CancellationToken::new();
    for _ in 0..2 {
        let result = collect_from(
            &source,
            &job,
            vec![Endpoint::get(
                "shared-inventory",
                "https://sqladmin.googleapis.com/v1/instances",
                "/items",
            )],
            &cancel,
        )
        .await;
        assert!(result.complete());
    }
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    Ok(())
}

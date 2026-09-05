//! Parallel inventory collection retains every independent result under a shared memory ceiling.
use monitor_core::{
    config::{resolve::Job, types::Config},
    model::{Check, Coverage},
};
use monitor_integrations::transport::Error;
use monitor_providers::common::{Endpoint, Source, collect_from};
use serde_json::{Value, json};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
#[derive(Default)]
struct Delayed {
    active: AtomicUsize,
    peak: AtomicUsize,
    calls: AtomicUsize,
    large: bool,
}
struct Active<'a>(&'a AtomicUsize);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
impl Source for Delayed {
    async fn request(
        &self,
        endpoint: &Endpoint,
        _: &Job,
        _: &CancellationToken,
    ) -> Result<Value, Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        let _active = Active(&self.active);
        tokio::time::sleep(Duration::from_millis(20)).await;
        if endpoint.id == "api-0" {
            return Err(Error::Denied);
        }
        if endpoint.id == "api-1" {
            return Err(Error::Authentication);
        }
        let rows: Vec<_> = (0..if self.large { 100 } else { 1 })
            .map(|index| json!({"name":format!("{}/{index}",endpoint.id),"state":"RUNNING"}))
            .collect();
        Ok(json!({"items":rows}))
    }
}
fn job() -> Result<Job, Box<dyn std::error::Error>> {
    Config::parse("version=1\n[settings]\nconcurrency=4\nscope_concurrency=4\n[[targets]]\nname='test'\nprovider='gcp'\nscope='fixture'")?.resolve(&Default::default())?.jobs.into_iter().find(|job| job.check == Check::Inventory).ok_or_else(||"job".into())
}
fn endpoints() -> Vec<Endpoint> {
    (0..12)
        .map(|index| {
            Endpoint::get(
                format!("api-{index}"),
                format!("https://example.googleapis.com/{index}"),
                "/items",
            )
        })
        .collect()
}
#[tokio::test(start_paused = true)]
async fn independent_failures_do_not_serialize_or_skip_other_apis()
-> Result<(), Box<dyn std::error::Error>> {
    let source = Delayed::default();
    let start = tokio::time::Instant::now();
    let result = collect_from(&source, &job()?, endpoints(), &CancellationToken::new()).await;
    assert_eq!(start.elapsed(), Duration::from_millis(60));
    assert_eq!(source.peak.load(Ordering::SeqCst), 4);
    assert_eq!(source.calls.load(Ordering::SeqCst), 12);
    assert_eq!(result.operations.len(), 12);
    assert_eq!(result.observations.len(), 10);
    assert!(
        result
            .operations
            .iter()
            .any(|op| op.coverage == Coverage::Denied)
    );
    assert!(
        result
            .operations
            .iter()
            .any(|op| op.coverage == Coverage::Unauthenticated)
    );
    Ok(())
}
#[tokio::test(start_paused = true)]
async fn intermediate_branches_do_not_multiply_the_evidence_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let mut job = job()?;
    job.settings.memory_bytes = 256 * 1024;
    let source = Delayed {
        large: true,
        ..Default::default()
    };
    let result = collect_from(&source, &job, endpoints(), &CancellationToken::new()).await;
    assert_eq!(source.peak.load(Ordering::SeqCst), 4);
    assert!(
        result
            .operations
            .iter()
            .any(|op| op.coverage == Coverage::Truncated)
    );
    assert!(
        monitor_core::bounds::result_bytes(&result)
            <= job.settings.memory_bytes / 2 / job.settings.concurrency + 4096
    );
    assert!(!result.complete());
    Ok(())
}

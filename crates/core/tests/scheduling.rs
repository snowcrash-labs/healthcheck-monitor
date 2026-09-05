//! Fake-time scheduling checks independent of provider availability.
use async_trait::async_trait;
use monitor_core::{
    config::{resolve::Selection, types::Config},
    model::*,
    scheduler::{Collector, Mode, drive},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
struct Fake {
    calls: AtomicUsize,
    active: AtomicUsize,
    max: AtomicUsize,
    delay: Duration,
}
#[async_trait]
impl Collector for Fake {
    async fn collect(
        &self,
        job: &monitor_core::config::resolve::Job,
        _: CancellationToken,
    ) -> CheckResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max.fetch_max(active, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        CheckResult::failure(
            job.target.name.clone(),
            job.check,
            job.revision.clone(),
            Coverage::Denied,
        )
    }
}
fn config() -> Result<monitor_core::config::resolve::Effective, Box<dyn std::error::Error>> {
    Ok(Config::parse("version=1\n[settings]\ninterval='1s'\nsamples=1\n[[targets]]\nname='a'\nprovider='gcp'\nscope='one'\n[[targets]]\nname='b'\nprovider='azure'\nscope='two'")?.resolve(&Selection::default())?)
}
#[tokio::test(start_paused = true)]
async fn simultaneous_failures_do_not_skip_independent_checks()
-> Result<(), Box<dyn std::error::Error>> {
    let effective = config()?;
    let expected = effective.jobs.len();
    let fake = Arc::new(Fake {
        calls: AtomicUsize::new(0),
        active: AtomicUsize::new(0),
        max: AtomicUsize::new(0),
        delay: Duration::from_millis(10),
    });
    let (_updates, rx) = watch::channel(effective);
    let (tx, mut results) = mpsc::channel(16);
    let task = tokio::spawn(drive(
        fake.clone(),
        rx,
        Mode::Once,
        tx,
        CancellationToken::new(),
    ));
    let mut count = 0;
    while results.recv().await.is_some() {
        count += 1;
    }
    task.await?;
    assert_eq!(count, expected);
    assert_eq!(fake.calls.load(Ordering::SeqCst), expected);
    assert!(fake.max.load(Ordering::SeqCst) <= 16);
    Ok(())
}
#[tokio::test(start_paused = true)]
async fn duration_expiry_and_slow_checks_never_overlap() -> Result<(), Box<dyn std::error::Error>> {
    let mut effective = config()?;
    effective.jobs.truncate(1);
    let fake = Arc::new(Fake {
        calls: AtomicUsize::new(0),
        active: AtomicUsize::new(0),
        max: AtomicUsize::new(0),
        delay: Duration::from_secs(3),
    });
    let (_updates, rx) = watch::channel(effective);
    let (tx, mut results) = mpsc::channel(16);
    let task = tokio::spawn(drive(
        fake.clone(),
        rx,
        Mode::Watch {
            duration: Some(Duration::from_secs(10)),
        },
        tx,
        CancellationToken::new(),
    ));
    while results.recv().await.is_some() {}
    task.await?;
    assert_eq!(fake.max.load(Ordering::SeqCst), 1);
    assert!(fake.calls.load(Ordering::SeqCst) <= 4);
    Ok(())
}
#[tokio::test(start_paused = true)]
async fn cancellation_finishes_partial_work() -> Result<(), Box<dyn std::error::Error>> {
    let effective = config()?;
    let fake = Arc::new(Fake {
        calls: AtomicUsize::new(0),
        active: AtomicUsize::new(0),
        max: AtomicUsize::new(0),
        delay: Duration::from_secs(60),
    });
    let (_updates, rx) = watch::channel(effective);
    let (tx, mut results) = mpsc::channel(16);
    let stop = CancellationToken::new();
    let task = tokio::spawn(drive(
        fake,
        rx,
        Mode::Watch { duration: None },
        tx,
        stop.clone(),
    ));
    tokio::task::yield_now().await;
    stop.cancel();
    while results.recv().await.is_some() {}
    task.await?;
    Ok(())
}

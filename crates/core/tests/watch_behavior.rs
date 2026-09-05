//! Simulated foreground runs verify scheduling and bounded state under prolonged churn.
use chrono::Utc;
use monitor_core::{
    config::{
        duration::Span,
        resolve::{Effective, Job, Selection},
        types::Config,
    },
    model::*,
    scheduler::{Collector, Mode, drive},
    state::State,
    storage::Store,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{mpsc, watch},
    time::Instant,
};
use tokio_util::sync::CancellationToken;
struct Harness {
    start: Instant,
    calls: AtomicUsize,
    active: AtomicUsize,
    peak: AtomicUsize,
    per_key: Mutex<BTreeMap<String, usize>>,
    overlap: AtomicUsize,
    delay: Duration,
}
struct Running<'a> {
    harness: &'a Harness,
    key: String,
}
impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.harness.active.fetch_sub(1, Ordering::SeqCst);
        if let Ok(mut active) = self.harness.per_key.lock() {
            active.remove(&self.key);
        }
    }
}
impl Harness {
    fn new(delay: Duration) -> Self {
        Self {
            start: Instant::now(),
            calls: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            per_key: Mutex::new(BTreeMap::new()),
            overlap: AtomicUsize::new(0),
            delay,
        }
    }
}
impl Collector for Harness {
    async fn collect(&self, job: &Job, _: CancellationToken) -> CheckResult {
        let cycle = self.calls.fetch_add(1, Ordering::SeqCst);
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        if let Ok(mut active) = self.per_key.lock()
            && active.insert(job.key.clone(), 1).is_some()
        {
            self.overlap.fetch_add(1, Ordering::SeqCst);
        }
        let _running = Running {
            harness: self,
            key: job.key.clone(),
        };
        tokio::time::sleep(self.delay).await;
        let at = Utc::now() + chrono::Duration::seconds(self.start.elapsed().as_secs() as i64);
        let mut result = CheckResult::failure(
            job.target.name.clone(),
            job.check,
            job.revision.clone(),
            if cycle.is_multiple_of(5) {
                Coverage::Denied
            } else {
                Coverage::Complete
            },
        );
        result.operations[0].id = "inventory".into();
        result.operations[0].observed_at = at;
        result.operations[0].records = 16;
        result.finished_at = at;
        for resource in 0..16 {
            result.observations.push(Observation {
                resource: format!("{}/inventory/{cycle}-{resource}", job.target.name),
                operation: "inventory".into(),
                observed_at: at,
                expected: Expected::Active,
                data: Data::Condition {
                    rule: "service-failed".into(),
                    healthy: Some(resource % 3 != 0),
                },
            });
        }
        result
    }
}
fn effective() -> Result<Effective, Box<dyn std::error::Error>> {
    let mut effective=Config::parse("version=1\n[settings]\nsamples=1\njitter_percent=0\n[[targets]]\nname='gcp'\nprovider='gcp'\nscope='one'\n[[targets]]\nname='azure'\nprovider='azure'\nscope='two'")?.resolve(&Selection::default())?;
    effective.jobs.retain(|job| job.check == Check::Inventory);
    Ok(effective)
}
#[tokio::test(start_paused = true)]
async fn reload_replaces_cadence_and_cancels_removed_checks_without_overlap()
-> Result<(), Box<dyn std::error::Error>> {
    let mut config = effective()?;
    for job in &mut config.jobs {
        job.settings.interval = Span(1);
    }
    let harness = Arc::new(Harness::new(Duration::from_secs(2)));
    let (updates, rx) = watch::channel(config.clone());
    let (tx, mut results) = mpsc::channel(16);
    let task = tokio::spawn(drive(
        harness.clone(),
        rx,
        Mode::Watch {
            duration: Some(Duration::from_secs(20)),
        },
        tx,
        CancellationToken::new(),
    ));
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(1)).await;
    config.jobs.truncate(1);
    config.revision = "replacement".into();
    config.jobs[0].revision = config.revision.clone();
    config.jobs[0].settings.interval = Span(5);
    updates.send(config)?;
    let mut seen = 0;
    while let Some((job, _)) = results.recv().await {
        assert_eq!(job.revision, "replacement");
        seen += 1;
    }
    task.await?;
    assert!((3..=5).contains(&seen));
    assert_eq!(harness.overlap.load(Ordering::SeqCst), 0);
    assert_eq!(harness.active.load(Ordering::SeqCst), 0);
    Ok(())
}
#[tokio::test(start_paused = true)]
async fn closed_reload_channel_does_not_spin_or_prevent_duration_expiry()
-> Result<(), Box<dyn std::error::Error>> {
    let (updates, rx) = watch::channel(effective()?);
    drop(updates);
    let (tx, mut results) = mpsc::channel(16);
    let task = tokio::spawn(drive(
        Arc::new(Harness::new(Duration::from_millis(1))),
        rx,
        Mode::Watch {
            duration: Some(Duration::from_secs(3)),
        },
        tx,
        CancellationToken::new(),
    ));
    while results.recv().await.is_some() {}
    task.await?;
    Ok(())
}
#[tokio::test(start_paused = true)]
async fn full_focused_single_sample_and_deep_runs_share_the_watch_engine()
-> Result<(), Box<dyn std::error::Error>> {
    let config = Config::parse("version=1\n[[targets]]\nname='gcp'\nprovider='gcp'\nscope='one'")?;
    for (profile, checks, samples) in [
        ("full", vec![], None),
        ("full", vec![Check::Queues], Some(1)),
        ("deep", vec![], None),
    ] {
        let selection = Selection {
            profile: Some(profile.into()),
            checks,
            overrides: monitor_core::config::settings::SettingsPatch {
                samples,
                ..Default::default()
            },
            ..Default::default()
        };
        let effective = config.resolve(&selection)?;
        let count: usize = effective
            .jobs
            .iter()
            .map(|job| job.settings.samples as usize)
            .sum();
        if profile == "deep" {
            assert!(
                effective
                    .jobs
                    .iter()
                    .all(|job| job.settings.log_entries == 5000
                        && job.settings.log_window == Span(86400))
            );
        }
        let (_updates, rx) = watch::channel(effective);
        let (tx, mut results) = mpsc::channel(16);
        let harness = Arc::new(Harness::new(Duration::from_millis(1)));
        let task = tokio::spawn(drive(
            harness.clone(),
            rx,
            Mode::Once,
            tx,
            CancellationToken::new(),
        ));
        let mut collected = 0;
        while results.recv().await.is_some() {
            collected += 1;
        }
        task.await?;
        assert_eq!(collected, count);
        assert_eq!(harness.overlap.load(Ordering::SeqCst), 0);
    }
    Ok(())
}
#[tokio::test(start_paused = true)]
async fn day_of_asset_churn_and_failures_respects_state_task_and_disk_bounds()
-> Result<(), Box<dyn std::error::Error>> {
    let mut effective = effective()?;
    for job in &mut effective.jobs {
        job.settings.interval = Span(30);
        job.settings.max_assets = 128;
        job.settings.max_findings = 64;
        job.settings.memory_bytes = 4 * 1024 * 1024;
        job.settings.history_count = 3;
        job.settings.history_bytes = 1024 * 1024;
    }
    let settings = effective.jobs[0].settings.clone();
    let harness = Arc::new(Harness::new(Duration::from_millis(1)));
    let mut state = State::new(
        effective.revision.clone(),
        effective.jobs.iter().map(|job| job.key.clone()).collect(),
    );
    let (_updates, rx) = watch::channel(effective);
    let (tx, mut results) = mpsc::channel(4);
    let task = tokio::spawn(drive(
        harness.clone(),
        rx,
        Mode::Watch {
            duration: Some(Duration::from_secs(86400)),
        },
        tx,
        CancellationToken::new(),
    ));
    let directory = tempfile::tempdir()?;
    let store = Store::open(directory.path())?;
    let mut count = 0usize;
    while let Some((job, result)) = results.recv().await {
        let at = result.finished_at;
        let transitions = state.apply(&job, result, at);
        count += 1;
        assert!(state.snapshot.findings.len() <= settings.max_findings);
        assert!(state.snapshot.retired.len() <= settings.max_findings);
        assert!(
            state
                .snapshot
                .results
                .values()
                .map(|r| r.observations.len())
                .sum::<usize>()
                <= settings.max_assets
        );
        if count.is_multiple_of(200) {
            assert!(serde_json::to_vec(&state.snapshot)?.len() < settings.memory_bytes);
            store.publish(&state.snapshot, &transitions, &settings)?;
        }
    }
    task.await?;
    assert!(count > 5000);
    assert_eq!(harness.active.load(Ordering::SeqCst), 0);
    assert_eq!(harness.overlap.load(Ordering::SeqCst), 0);
    assert!(harness.peak.load(Ordering::SeqCst) <= 2);
    let bytes = std::fs::read_dir(directory.path())?
        .map(|entry| {
            entry
                .and_then(|entry| entry.metadata())
                .map(|metadata| metadata.len())
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .sum::<u64>();
    assert!(bytes <= settings.history_bytes);
    Ok(())
}

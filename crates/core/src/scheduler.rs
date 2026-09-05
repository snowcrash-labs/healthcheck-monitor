//! Fair scheduling with coalesced ticks, bounded tasks, and cancellation.
use crate::{
    config::resolve::{Effective, Job},
    model::{CheckResult, Coverage},
};
use futures::FutureExt;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{mpsc, watch},
    task::JoinSet,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

pub trait Collector: Send + Sync {
    fn collect(
        &self,
        job: &Job,
        cancel: CancellationToken,
    ) -> impl std::future::Future<Output = CheckResult> + Send;
}
#[derive(Debug, Clone, Copy)]
pub enum Mode {
    Once,
    Watch { duration: Option<Duration> },
}
struct Entry {
    job: Job,
    due: Instant,
    samples: u32,
    cancel: CancellationToken,
}
fn cadence(interval: Duration, percent: u8, key: &str, cycle: u32) -> Duration {
    let spread = interval.as_secs().saturating_mul(u64::from(percent)) / 100;
    if spread == 0 {
        return interval;
    }
    let hash = key.bytes().fold(u64::from(cycle), |hash, byte| {
        hash.wrapping_mul(1099511628211)
            .wrapping_add(u64::from(byte))
    });
    let delta = (hash % (spread * 2 + 1)) as i64 - spread as i64;
    Duration::from_secs(interval.as_secs().saturating_add_signed(delta).max(1))
}

/// A scope is admitted before spawning; throttled scopes cannot occupy waiting workers.
pub async fn drive<C: Collector + 'static>(
    collector: Arc<C>,
    mut config: watch::Receiver<Effective>,
    mode: Mode,
    output: mpsc::Sender<(Job, CheckResult)>,
    stop: CancellationToken,
) {
    let start = Instant::now();
    let mut entries: Vec<Entry> = config
        .borrow()
        .jobs
        .iter()
        .map(|job| Entry {
            job: job.clone(),
            due: start,
            samples: 0,
            cancel: stop.child_token(),
        })
        .collect();
    let mut running = BTreeSet::new();
    let mut scopes: BTreeMap<String, usize> = BTreeMap::new();
    let mut tasks = JoinSet::new();
    let mut cursor = 0;
    let mut config_open = true;
    loop {
        let expired = matches!(mode, Mode::Watch { duration: Some(d) } if start.elapsed() >= d);
        if stop.is_cancelled() || expired {
            break;
        }
        let global_limit = entries
            .iter()
            .map(|e| e.job.settings.concurrency)
            .min()
            .unwrap_or(1);
        let now = Instant::now();
        for offset in 0..entries.len() {
            let index = (cursor + offset) % entries.len();
            let entry = &mut entries[index];
            let scope = entry.job.scope();
            if running.len() >= global_limit {
                break;
            }
            if entry.due > now
                || running.contains(&entry.job.key)
                || scopes.get(&scope).copied().unwrap_or(0) >= entry.job.settings.scope_concurrency
                || matches!(mode, Mode::Once) && entry.samples >= entry.job.settings.samples
            {
                continue;
            }
            running.insert(entry.job.key.clone());
            *scopes.entry(scope).or_default() += 1;
            let job = entry.job.clone();
            let collector = collector.clone();
            let cancel = entry.cancel.child_token();
            let next_interval = match mode {
                Mode::Once => job.settings.sample_interval.duration(),
                Mode::Watch { .. } => job.settings.interval.duration(),
            };
            let jittered = if matches!(mode, Mode::Watch { .. }) {
                cadence(
                    next_interval,
                    job.settings.jitter_percent,
                    &job.key,
                    entry.samples,
                )
            } else {
                next_interval
            };
            entry.due = now + jittered;
            entry.samples = entry.samples.saturating_add(1);
            tasks.spawn(async move {
                let started = chrono::Utc::now();
                let result = tokio::select! {
                    _ = cancel.cancelled() => CheckResult::failure(job.target.name.clone(), job.check, job.revision.clone(), Coverage::Cancelled),
                    result = std::panic::AssertUnwindSafe(collector.collect(&job, cancel.clone())).catch_unwind() => match result {
                        Ok(result) => result,
                        Err(_) => CheckResult::failure(job.target.name.clone(), job.check, job.revision.clone(), Coverage::Malformed),
                    }
                };
                let mut result = result;
                result.started_at = started;
                (job, result)
            });
        }
        if !entries.is_empty() {
            cursor = (cursor + 1) % entries.len();
        }
        if matches!(mode, Mode::Once)
            && running.is_empty()
            && entries.iter().all(|e| e.samples >= e.job.settings.samples)
        {
            break;
        }
        let next_due = entries
            .iter()
            .filter(|e| {
                !running.contains(&e.job.key)
                    && !(matches!(mode, Mode::Once) && e.samples >= e.job.settings.samples)
            })
            .map(|e| e.due)
            .min()
            .unwrap_or(now + Duration::from_secs(1));
        let delay = next_due
            .saturating_duration_since(Instant::now())
            .clamp(Duration::from_millis(10), Duration::from_secs(1));
        tokio::select! {
            _ = stop.cancelled() => break,
            _ = tokio::time::sleep(delay) => {},
            changed = config.changed(), if config_open && matches!(mode, Mode::Watch { .. }) => {
                if changed.is_ok() {
                    let effective = config.borrow_and_update().clone();
                    let keys: BTreeSet<_> = effective.jobs.iter().map(|j| &j.key).collect();
                    for entry in &entries { if !keys.contains(&entry.job.key) { entry.cancel.cancel(); } }
                    entries = effective.jobs.into_iter().map(|job| {
                        let old = entries.iter().find(|e| e.job.key == job.key);
                        let cancel = old.map_or_else(|| stop.child_token(), |e| e.cancel.clone());
                        Entry { due: Instant::now(), samples: 0, job, cancel }
                    }).collect();
                } else {
                    config_open = false;
                }
            },
            completed = tasks.join_next(), if !tasks.is_empty() => {
                if let Some(Ok((job, result))) = completed {
                    running.remove(&job.key);
                    if let Some(count) = scopes.get_mut(&job.scope()) { *count = count.saturating_sub(1); }
                    if scopes.get(&job.scope()) == Some(&0) { scopes.remove(&job.scope()); }
                    if entries.iter().any(|e| e.job.key == job.key && e.job.revision == job.revision) {
                        tokio::select! {
                            _ = stop.cancelled() => break,
                            sent = output.send((job, result)) => if sent.is_err() { break; },
                        }
                    }
                }
            },
        }
    }
    for entry in &entries {
        entry.cancel.cancel();
    }
    let cleanup = async {
        while let Some(completed) = tasks.join_next().await {
            if let Ok((job, result)) = completed {
                let _ = output.send((job, result)).await;
            }
        }
    };
    if tokio::time::timeout(Duration::from_secs(5), cleanup)
        .await
        .is_err()
    {
        tasks.abort_all();
    }
}

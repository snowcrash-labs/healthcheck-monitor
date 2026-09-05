//! Fair scheduling with coalesced ticks, bounded tasks, and cancellation.
use crate::{
    config::resolve::{Effective, Job},
    model::{CheckResult, Coverage},
};
use async_trait::async_trait;
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

#[async_trait]
pub trait Collector: Send + Sync {
    async fn collect(&self, job: &Job, cancel: CancellationToken) -> CheckResult;
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

/// A scope is admitted before spawning; throttled scopes cannot occupy waiting workers.
pub async fn drive(
    collector: Arc<dyn Collector>,
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
            entry.due = now + next_interval;
            entry.samples += 1;
            tasks.spawn(async move {
                let started = chrono::Utc::now();
                let result = tokio::select! {
                    _ = cancel.cancelled() => CheckResult::failure(job.target.name.clone(), job.check, job.revision.clone(), Coverage::Cancelled),
                    result = tokio::time::timeout(job.settings.operation_timeout.duration(), collector.collect(&job, cancel.clone())) => match result {
                        Ok(result) => result,
                        Err(_) => CheckResult::failure(job.target.name.clone(), job.check, job.revision.clone(), Coverage::Timeout),
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
            changed = config.changed(), if matches!(mode, Mode::Watch { .. }) => {
                if changed.is_ok() {
                    let effective = config.borrow_and_update().clone();
                    let keys: BTreeSet<_> = effective.jobs.iter().map(|j| &j.key).collect();
                    for entry in &entries { if !keys.contains(&entry.job.key) { entry.cancel.cancel(); } }
                    entries = effective.jobs.into_iter().map(|job| {
                        let old = entries.iter().find(|e| e.job.key == job.key);
                        let cancel = old.map_or_else(|| stop.child_token(), |e| e.cancel.clone());
                        Entry { due: Instant::now(), samples: 0, job, cancel }
                    }).collect();
                }
            },
            completed = tasks.join_next(), if !tasks.is_empty() => {
                if let Some(Ok((job, result))) = completed {
                    running.remove(&job.key);
                    if let Some(count) = scopes.get_mut(&job.scope()) { *count = count.saturating_sub(1); }
                    if entries.iter().any(|e| e.job.key == job.key && e.job.revision == job.revision) {
                        if output.send((job, result)).await.is_err() { break; }
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

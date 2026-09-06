//! Nonblocking journal admission accounts for records before they enter a bounded queue.
use crate::{
    error::Error,
    pool::History,
    records::{Event, Gap, Run},
    types::Digest,
};
use monitor_core::budget::{Budget, Permit};
use serde::Serialize;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub struct Journal {
    pub(crate) sender: mpsc::Sender<Batch>,
    pub(crate) bytes: Arc<Budget>,
    pub(crate) status: Arc<Status>,
}
pub(crate) struct Batch {
    pub revision: Digest,
    pub runs: Vec<Run>,
    pub events: Vec<Event>,
    pub records: Vec<crate::query_records::QueryRecord>,
    pub gaps: Vec<Gap>,
    pub _charges: Vec<Permit>,
    pub _pending: Pending,
}
/// Count queued and in-flight work together so shutdown cannot race a dequeue.
pub(crate) struct Pending(Arc<Status>);
impl Pending {
    pub fn new(status: &Arc<Status>) -> Self {
        status.outstanding.fetch_add(1, Ordering::AcqRel);
        Self(status.clone())
    }
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.0.outstanding.fetch_sub(1, Ordering::AcqRel);
    }
}
#[derive(Default)]
pub(crate) struct Status {
    pub available: AtomicBool,
    pub outstanding: AtomicU64,
    pub last_write: AtomicI64,
    pub dropped_events: AtomicU64,
    pub dropped_runs: AtomicU64,
    pub pending_events: AtomicU64,
    pub pending_runs: AtomicU64,
    pub gaps: AtomicU64,
}
#[derive(Serialize)]
pub struct Health {
    pub available: bool,
    pub last_persisted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub dropped_events: u64,
    pub dropped_runs: u64,
    pub queued_batches: usize,
    pub gaps: u64,
}
impl Journal {
    pub fn start(
        history: Arc<History>,
        stop: CancellationToken,
    ) -> (Arc<Self>, tokio::task::JoinHandle<()>) {
        let (sender, receiver) = mpsc::channel(history.config.queue_batches);
        let journal = Arc::new(Self {
            sender,
            bytes: Arc::new(Budget::new(history.config.queue_bytes)),
            status: Arc::new(Status::default()),
        });
        let status = journal.status.clone();
        let task = tokio::spawn(async move {
            crate::journal_worker::run(history, receiver, status.clone(), stop).await;
            status.available.store(false, Ordering::Release);
        });
        (journal, task)
    }
    pub fn health(&self) -> Health {
        let at = self.status.last_write.load(Ordering::Acquire);
        Health {
            available: self.status.available.load(Ordering::Acquire),
            last_persisted_at: (at != 0)
                .then(|| chrono::DateTime::from_timestamp_millis(at))
                .flatten(),
            dropped_events: self.status.dropped_events.load(Ordering::Relaxed),
            dropped_runs: self.status.dropped_runs.load(Ordering::Relaxed),
            queued_batches: self.sender.max_capacity() - self.sender.capacity(),
            gaps: self.status.gaps.load(Ordering::Relaxed),
        }
    }
    pub async fn flush(&self, deadline: tokio::time::Instant) -> bool {
        while tokio::time::Instant::now() < deadline {
            if self.status.outstanding.load(Ordering::Acquire) == 0
                && self.status.pending_events.load(Ordering::Acquire) == 0
                && self.status.pending_runs.load(Ordering::Acquire) == 0
            {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        false
    }
    fn reserve(&self, value: &impl Serialize) -> Result<Permit, Error> {
        let bytes = serde_json::to_vec(value)
            .map_err(|_| Error::Record)?
            .len()
            .saturating_mul(2)
            .saturating_add(512);
        self.bytes
            .clone()
            .try_acquire_many_owned(u32::try_from(bytes).map_err(|_| Error::Capacity)?)
            .map_err(|_| Error::Capacity)
    }
    pub fn submit(
        &self,
        revision: Digest,
        runs: impl IntoIterator<Item = Result<Run, Error>>,
        events: impl IntoIterator<Item = Result<Event, Error>>,
    ) {
        self.submit_queries(revision, runs, events, std::iter::empty());
    }
    pub fn submit_queries(
        &self,
        revision: Digest,
        runs: impl IntoIterator<Item = Result<Run, Error>>,
        events: impl IntoIterator<Item = Result<Event, Error>>,
        records: impl IntoIterator<Item = Result<crate::query_records::QueryRecord, Error>>,
    ) {
        let mut batch = Batch {
            revision,
            runs: vec![],
            events: vec![],
            records: vec![],
            gaps: vec![],
            _charges: vec![],
            _pending: Pending::new(&self.status),
        };
        for run in runs {
            match run.and_then(|run| self.reserve(&run).map(|permit| (run, permit))) {
                Ok((run, permit)) if batch.runs.len() < 2048 => {
                    batch.runs.push(run);
                    batch._charges.push(permit);
                }
                _ => self.status.drop_records(0, 1),
            }
        }
        for event in events {
            match event.and_then(|event| self.reserve(&event).map(|permit| (event, permit))) {
                Ok((event, permit)) if batch.events.len() < 10000 => {
                    batch.events.push(event);
                    batch._charges.push(permit);
                }
                _ => self.status.drop_records(1, 0),
            }
        }
        for record in records {
            match record.and_then(|record| self.reserve(&record).map(|permit| (record, permit))) {
                Ok((record, permit)) if batch.records.len() < 20000 => {
                    batch.records.push(record);
                    batch._charges.push(permit);
                }
                _ => self.status.drop_records(1, 0),
            }
        }
        if batch.records.is_empty()
            && batch.runs.is_empty()
            && batch.events.is_empty()
            && self.status.pending_events.load(Ordering::Relaxed) == 0
            && self.status.pending_runs.load(Ordering::Relaxed) == 0
        {
            return;
        }
        if let Err(error) = self.sender.try_send(batch) {
            let batch = error.into_inner();
            self.status.drop_records(
                (batch.events.len() + batch.records.len()) as u64,
                batch.runs.len() as u64,
            );
        }
    }
}
impl Status {
    pub fn drop_records(&self, events: u64, runs: u64) {
        for (counter, amount) in [
            (&self.dropped_events, events),
            (&self.pending_events, events),
            (&self.dropped_runs, runs),
            (&self.pending_runs, runs),
        ] {
            let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                Some(value.saturating_add(amount))
            });
        }
        self.available.store(false, Ordering::Release);
    }
}

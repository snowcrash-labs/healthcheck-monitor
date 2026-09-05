//! Shared fingerprint cache reserves a finite portion of the retained-state budget.
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
pub struct Dedupe {
    entries: scc::HashCache<[u8; 32], DateTime<Utc>>,
    _bytes: OwnedSemaphorePermit,
}
impl Dedupe {
    pub fn new(limit: usize, bytes: Arc<Semaphore>) -> Option<Self> {
        let size = u32::try_from(limit.saturating_add(32).saturating_mul(256)).ok()?;
        let permit = bytes.try_acquire_many_owned(size).ok()?;
        Some(Self {
            entries: scc::HashCache::with_capacity(0, limit),
            _bytes: permit,
        })
    }
    /// One log check runs per target; entries never contain payloads or original event IDs.
    pub fn accept(
        &self,
        fingerprint: [u8; 32],
        batch_end: DateTime<Utc>,
        committed: Option<DateTime<Utc>>,
    ) -> bool {
        if self
            .entries
            .read_sync(&fingerprint, |_, at| {
                committed.is_some_and(|committed| *at <= committed)
            })
            .unwrap_or(false)
        {
            return false;
        }
        let (_, mut entry) = self.entries.entry_sync(fingerprint).or_put(batch_end);
        *entry.get_mut() = batch_end;
        true
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

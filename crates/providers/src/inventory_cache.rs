//! Bounded normalized inventory cache with coalesced concurrent reads.
use crate::{
    common::{Endpoint, Source},
    endpoint_scan::{Fetched, fetch},
};
use monitor_core::{
    config::resolve::Job,
    model::{Check, CheckResult, Coverage},
};
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;
struct Entry {
    consumers: std::collections::BTreeSet<Check>,
    at: tokio::time::Instant,
    value: Fetched,
    _bytes: OwnedSemaphorePermit,
}
pub struct InventoryCache {
    pub(crate) tokens: crate::registry_tokens::Tokens,
    entries: scc::HashCache<String, Arc<Mutex<Option<Entry>>>>,
    bytes: Arc<Semaphore>,
}
impl InventoryCache {
    pub fn new(entries: usize, bytes: Arc<Semaphore>) -> Self {
        Self {
            tokens: crate::registry_tokens::Tokens::new(bytes.clone()),
            entries: scc::HashCache::with_capacity(0, entries),
            bytes,
        }
    }
    pub async fn load<S: Source>(
        &self,
        source: &S,
        job: &Job,
        endpoint: Endpoint,
        cancel: &CancellationToken,
        key: String,
    ) -> Fetched {
        let slot = {
            let (_, entry) = self
                .entries
                .entry_async(key)
                .await
                .or_put_with(|| Arc::new(Mutex::new(None)));
            entry.get().clone()
        };
        let mut entry = tokio::select! {
            _=cancel.cancelled()=>return Fetched { result:CheckResult::failure(job.target.name.clone(),job.check,job.revision.clone(),Coverage::Cancelled),followups:vec![] },
            entry=slot.lock()=>entry,
        };
        if let Some(cached) = entry.as_mut().filter(|entry| {
            entry.at.elapsed() < job.settings.interval.duration()
                && !entry.consumers.contains(&job.check)
        }) {
            cached.consumers.insert(job.check);
            return cached.value.clone();
        }
        *entry = None;
        let value = fetch(source, job, endpoint, cancel).await;
        if value
            .result
            .operations
            .iter()
            .all(|op| op.coverage == Coverage::Complete)
        {
            let bytes = monitor_core::bounds::result_bytes(&value.result).saturating_add(
                value
                    .followups
                    .iter()
                    .map(|endpoint| endpoint.url.len().saturating_mul(4).saturating_add(4096))
                    .sum::<usize>(),
            );
            if let Ok(bytes) = u32::try_from(bytes)
                && let Ok(permit) = self.bytes.clone().try_acquire_many_owned(bytes)
            {
                *entry = Some(Entry {
                    consumers: std::collections::BTreeSet::from([job.check]),
                    at: tokio::time::Instant::now(),
                    value: value.clone(),
                    _bytes: permit,
                });
            }
        }
        value
    }
}

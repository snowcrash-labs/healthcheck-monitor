//! Bounded, expiring scope-response storage and per-key request coalescing stay inside the process.
use crate::response::ApiError;
use axum::{
    body::Bytes,
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::{sync::Mutex, time::Instant};

const MAX_ENTRIES: usize = 64;
const MAX_BYTES: usize = 32 * 1024 * 1024;
const TTL: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Key {
    pub generation: u64,
    pub filter: String,
}
#[derive(Clone)]
pub struct Cached {
    pub body: Bytes,
    created: Instant,
    ttl: Duration,
}
impl Cached {
    /// Timestamp a historical response without sliding its lifetime on subsequent reads.
    pub fn new(body: Bytes) -> Self {
        Self {
            body,
            created: Instant::now(),
            ttl: TTL,
        }
    }
    /// Briefly share degraded pages so a database outage does not serialize a burst of retries.
    pub fn current_only(body: Bytes) -> Self {
        Self {
            body,
            created: Instant::now(),
            ttl: Duration::from_secs(1),
        }
    }
    /// Share immutable bytes while reporting their actual cache age.
    pub fn response(&self, hit: bool) -> Result<Response, ApiError> {
        let mut response = (
            [
                (header::CONTENT_TYPE, "application/json"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            self.body.clone(),
        )
            .into_response();
        response.headers_mut().insert(
            "x-healthcheck-cache",
            HeaderValue::from_static(if hit { "hit" } else { "miss" }),
        );
        response.headers_mut().insert(
            header::AGE,
            HeaderValue::from_str(&self.created.elapsed().as_secs().to_string())
                .map_err(|_| ApiError::Unavailable)?,
        );
        Ok(response)
    }
}
struct Entry {
    key: Key,
    value: Cached,
}
impl Entry {
    fn bytes(&self) -> usize {
        self.key.filter.len() + self.value.body.len()
    }
}
#[derive(Default)]
struct State {
    entries: VecDeque<Entry>,
    bytes: usize,
    flights: HashMap<Key, Weak<Mutex<()>>>,
}
impl State {
    fn expire(&mut self) {
        self.entries
            .retain(|entry| entry.value.created.elapsed() < entry.value.ttl);
        self.bytes = self.entries.iter().map(Entry::bytes).sum();
        self.flights.retain(|_, flight| flight.strong_count() > 0);
    }
}
#[derive(Default)]
pub struct Cache {
    state: Mutex<State>,
}
impl Cache {
    /// Return a fresh entry and promote it in the bounded eviction order.
    pub async fn get(&self, key: &Key) -> Option<Cached> {
        let mut state = self.state.lock().await;
        state.expire();
        let index = state.entries.iter().position(|entry| &entry.key == key)?;
        let entry = state.entries.remove(index)?;
        let value = entry.value.clone();
        state.entries.push_back(entry);
        Some(value)
    }
    /// Supersede an existing value even when its replacement cannot fit the cache.
    pub async fn insert(&self, key: Key, value: Cached) {
        let entry = Entry { key, value };
        let mut state = self.state.lock().await;
        state.expire();
        if let Some(index) = state.entries.iter().position(|old| old.key == entry.key)
            && let Some(old) = state.entries.remove(index)
        {
            state.bytes -= old.bytes();
        }
        if entry.bytes() > MAX_BYTES {
            return;
        }
        while state.entries.len() >= MAX_ENTRIES || state.bytes + entry.bytes() > MAX_BYTES {
            if let Some(old) = state.entries.pop_front() {
                state.bytes -= old.bytes();
            } else {
                break;
            }
        }
        state.bytes += entry.bytes();
        state.entries.push_back(entry);
    }
    /// Share loads for a key without retaining locks after their callers finish.
    pub async fn flight(&self, key: &Key) -> Result<Arc<Mutex<()>>, ApiError> {
        let mut state = self.state.lock().await;
        state.expire();
        if let Some(flight) = state.flights.get(key).and_then(Weak::upgrade) {
            return Ok(flight);
        }
        if state.flights.len() >= MAX_ENTRIES {
            return Err(ApiError::Busy);
        }
        let flight = Arc::new(Mutex::new(()));
        state.flights.insert(key.clone(), Arc::downgrade(&flight));
        Ok(flight)
    }
}

//! A single background disk writer with coalesced requests and bounded retry backoff.
use crate::{
    config::settings::Settings,
    error::Error,
    model::{Snapshot, Transition},
    storage::Store,
};
use std::{sync::Arc, time::Duration};
use tokio::{task::JoinHandle, time::Instant};
pub trait Sink: Send + Sync + 'static {
    fn publish(
        &self,
        snapshot: &Snapshot,
        transitions: &[Transition],
        settings: &Settings,
    ) -> Result<(), Error>;
}
impl Sink for Store {
    fn publish(
        &self,
        snapshot: &Snapshot,
        transitions: &[Transition],
        settings: &Settings,
    ) -> Result<(), Error> {
        Store::publish(self, snapshot, transitions, settings)
    }
}
pub struct Publisher<W: Sink> {
    sink: Arc<W>,
    task: Option<JoinHandle<Result<(), Error>>>,
    event_count: usize,
    requested: bool,
    next: Instant,
    backoff: Duration,
}
impl<W: Sink> Publisher<W> {
    pub fn new(sink: Arc<W>) -> Self {
        Self {
            sink,
            task: None,
            event_count: 0,
            requested: false,
            next: Instant::now(),
            backoff: Duration::from_secs(1),
        }
    }
    /// Requests made during a write coalesce into one later publication of current state.
    pub fn request(&mut self) {
        self.requested = true;
    }
    pub fn start(
        &mut self,
        snapshot: &Snapshot,
        transitions: &[Transition],
        settings: &Settings,
    ) -> bool {
        if self.task.is_some() || !self.requested || self.next > Instant::now() {
            return false;
        }
        self.requested = false;
        self.event_count = transitions.len();
        let sink = self.sink.clone();
        let mut snapshot = snapshot.clone();
        snapshot.captured_at = chrono::Utc::now();
        snapshot.persistence_fault = false;
        let transitions = transitions.to_vec();
        let settings = settings.clone();
        self.task = Some(tokio::task::spawn_blocking(move || {
            sink.publish(&snapshot, &transitions, &settings)
        }));
        true
    }
    /// Successful writes acknowledge only their captured prefix; later events remain queued.
    pub async fn poll(&mut self) -> Option<Result<usize, ()>> {
        if !self.task.as_ref().is_some_and(JoinHandle::is_finished) {
            return None;
        }
        self.complete().await
    }
    async fn complete(&mut self) -> Option<Result<usize, ()>> {
        let task = self.task.take()?;
        match task.await {
            Ok(Ok(())) => {
                self.next = Instant::now();
                self.backoff = Duration::from_secs(1);
                Some(Ok(self.event_count))
            }
            _ => {
                self.requested = true;
                self.next = Instant::now() + self.backoff;
                self.backoff = (self.backoff * 2).min(Duration::from_secs(60));
                Some(Err(()))
            }
        }
    }
    /// Drain a prior write and publish the final state under one caller-supplied deadline.
    pub async fn finish(
        &mut self,
        snapshot: &Snapshot,
        transitions: &mut Vec<Transition>,
        settings: &Settings,
        deadline: Instant,
    ) -> bool {
        if Instant::now() >= deadline {
            return false;
        }
        if self.task.is_some() {
            match tokio::time::timeout_at(deadline, self.complete()).await {
                Ok(Some(Ok(count))) => {
                    transitions.drain(..count.min(transitions.len()));
                }
                Ok(_) => {}
                Err(_) => return false,
            }
        }
        self.request();
        self.next = Instant::now();
        if !self.start(snapshot, transitions, settings) {
            return false;
        }
        match tokio::time::timeout_at(deadline, self.complete()).await {
            Ok(Some(Ok(count))) => {
                transitions.drain(..count.min(transitions.len()));
                true
            }
            _ => false,
        }
    }
}
#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;

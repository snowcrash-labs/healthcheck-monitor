//! One immutable view and coalesced revision channel serve all dashboard readers.
use crate::view::View;
use monitor_core::{
    config::resolve::Effective,
    model::{Snapshot, Transition},
};
use monitor_history::{
    journal::Journal,
    records::{Event, Run},
    types::Digest,
};
use monitor_runtime::Observer;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
};
pub struct Bus {
    views: scc::HashMap<(), Arc<View>>,
    recorder: scc::HashMap<(), crate::query_recorder::Recorder>,
    generation: AtomicU64,
    pub changed: tokio::sync::watch::Sender<u64>,
    pub journal: Arc<Journal>,
    pub heartbeat: AtomicI64,
    pub running: AtomicBool,
}
impl Bus {
    pub fn new(journal: Arc<Journal>) -> Arc<Self> {
        let (changed, _) = tokio::sync::watch::channel(0);
        Arc::new(Self {
            views: scc::HashMap::new(),
            recorder: scc::HashMap::new(),
            generation: AtomicU64::new(0),
            changed,
            journal,
            heartbeat: AtomicI64::new(0),
            running: AtomicBool::new(true),
        })
    }
    pub fn current(&self) -> Option<Arc<View>> {
        self.views.read_sync(&(), |_, view| view.clone())
    }
}
impl Observer for Bus {
    fn update(&self, snapshot: &Snapshot, effective: &Effective, transitions: &[Transition]) {
        let previous = self.current();
        if previous.as_ref().is_some_and(|view| {
            view.captured_at == snapshot.captured_at
                && view.revision == snapshot.revision
                && view.persistence_fault == snapshot.persistence_fault
        }) && transitions.is_empty()
        {
            return;
        }
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let view = crate::build_view::build(snapshot, effective, generation);
        if let Ok(revision) = Digest::try_new(snapshot.revision.clone()) {
            let runs = snapshot
                .results
                .iter()
                .filter(|(key, result)| {
                    result.revision == snapshot.revision
                        && previous
                            .as_ref()
                            .and_then(|view| view.result_stamps.get(*key))
                            .is_none_or(|stamp| !stamp.matches(result))
                })
                .map(|(_, result)| Run::new(result));
            let events = transitions.iter().map(|transition| {
                if let Some(finding) = snapshot.findings.get(&transition.finding) {
                    return Event::new(
                        &crate::build_view::target(&finding.resource, &view.targets),
                        transition,
                        finding,
                    );
                }
                let old = previous
                    .as_ref()
                    .and_then(|view| {
                        view.findings
                            .binary_search_by(|finding| finding.id.cmp(&transition.finding))
                            .ok()
                            .and_then(|index| view.findings.get(index))
                    })
                    .ok_or(monitor_history::error::Error::Record)?;
                Event::new(&old.target, transition, &old.source())
            });
            let mut recorder = self
                .recorder
                .entry_sync(())
                .or_insert_with(Default::default);
            recorder
                .get_mut()
                .retain(&view.resources.iter().map(|r| r.id.as_str()).collect());
            let records = crate::query_publish::records(
                recorder.get_mut(),
                snapshot,
                effective,
                &view,
                previous.as_deref(),
                transitions,
            );
            self.journal.submit_queries(revision, runs, events, records);
        }
        let view = Arc::new(view);
        let mut entry = self.views.entry_sync(()).or_insert_with(|| view.clone());
        *entry.get_mut() = view;
        drop(entry);
        self.changed.send_replace(generation);
    }
    fn heartbeat(&self, at: chrono::DateTime<chrono::Utc>, running: bool) {
        self.heartbeat
            .store(at.timestamp_millis(), Ordering::Release);
        self.running.store(running, Ordering::Release);
    }
}

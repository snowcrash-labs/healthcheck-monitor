//! Diagnostic classes discard raw messages, customer values, and stack payloads.
use chrono::{DateTime, Utc};
use monitor_core::model::{Data, LogClass};
use std::collections::BTreeMap;
/// Only a fixed diagnostic vocabulary crosses into evidence.
pub fn classify(message: &str) -> LogClass {
    let message = message.to_ascii_lowercase();
    if message.contains("userwarning")
        || message.contains("futurewarning")
        || message.contains("runtimewarning")
    {
        LogClass::Warning
    } else if message.contains("importerror") || message.contains("modulenotfounderror") {
        LogClass::Import
    } else if message.contains("out of memory") || message.contains("oomkilled") {
        LogClass::OutOfMemory
    } else if message.contains("panic") || message.contains("traceback") {
        LogClass::Panic
    } else if message.contains("permission denied") || message.contains("unauthorized") {
        LogClass::Permission
    } else if message.contains("timeout") || message.contains("timed out") {
        LogClass::Timeout
    } else if message.contains("connection") || message.contains("unavailable") {
        LogClass::Connection
    } else {
        LogClass::OtherError
    }
}
#[derive(Default)]
pub struct Groups {
    entries: BTreeMap<LogClass, (u64, DateTime<Utc>, DateTime<Utc>)>,
}
impl Groups {
    pub fn add(&mut self, message: &str, at: DateTime<Utc>) {
        let entry = self.entries.entry(classify(message)).or_insert((0, at, at));
        entry.0 = entry.0.saturating_add(1);
        entry.1 = entry.1.min(at);
        entry.2 = entry.2.max(at);
    }
    pub fn finish(self) -> Vec<Data> {
        self.entries
            .into_iter()
            .map(|(signature, (count, first_seen, last_seen))| Data::Log {
                signature,
                count,
                first_seen,
                last_seen,
                sampled: true,
            })
            .collect()
    }
}

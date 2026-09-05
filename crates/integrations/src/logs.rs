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
    entries: BTreeMap<(String, LogClass), Group>,
}
struct Group {
    count: u64,
    first: DateTime<Utc>,
    last: DateTime<Utc>,
}
impl Groups {
    pub fn add(&mut self, message: &str, at: DateTime<Utc>) {
        self.add_for("unknown", message, at);
    }
    pub fn add_for(&mut self, scope: &str, message: &str, at: DateTime<Utc>) {
        let entry = self
            .entries
            .entry((super::projection::identity(scope), classify(message)))
            .or_insert(Group {
                count: 0,
                first: at,
                last: at,
            });
        entry.count = entry.count.saturating_add(1);
        entry.first = entry.first.min(at);
        entry.last = entry.last.max(at);
    }
    pub fn finish(self) -> Vec<Data> {
        self.finish_scoped()
            .into_iter()
            .map(|(_, data)| data)
            .collect()
    }
    pub fn finish_scoped(self) -> Vec<(String, Data)> {
        self.entries
            .into_iter()
            .map(|((scope, signature), group)| {
                (
                    format!("{scope}/{signature:?}"),
                    Data::Log {
                        signature,
                        count: group.count,
                        first_seen: group.first,
                        last_seen: group.last,
                        sampled: true,
                    },
                )
            })
            .collect()
    }
}

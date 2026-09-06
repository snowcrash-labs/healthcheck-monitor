//! Compact unchanged findings into temporal intervals; source observations retain their timestamps.
use monitor_history::{error::Error, query_records::QueryRecord, records::digest, types::Digest};
use monitor_query::{
    enums::*,
    record::{Details, Record},
};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Recorder {
    active: BTreeMap<String, Version>,
    bytes: usize,
}
struct Version {
    fingerprint: Digest,
    row: QueryRecord,
    bytes: usize,
}
impl Recorder {
    /// This map is bounded independently of snapshots and never contains provider payloads.
    pub fn capture(
        &mut self,
        mut record: Record,
        at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<QueryRecord>, Error> {
        if matches!(record.category(), Category::Check | Category::Diagnostic) {
            let key = digest(&(
                record.category(),
                &record.identity,
                record.observed_at,
                &record.details,
            ))?;
            return Ok(vec![QueryRecord::new(key, record)?]);
        }
        let identity = format!("{:?}/{}", record.category(), record.identity);
        let fingerprint = digest(&(
            &record.scope,
            &record.location,
            record.check,
            &record.resource,
            record.stale,
            &record.details,
        ))?;
        let bytes = serde_json::to_vec(&record)
            .map_err(|_| Error::Record)?
            .len()
            .saturating_mul(2)
            .saturating_add(1024);
        let previous_bytes = self.active.get(&identity).map_or(0, |v| v.bytes);
        if self
            .bytes
            .saturating_sub(previous_bytes)
            .saturating_add(bytes)
            > 16 * 1024 * 1024
        {
            return Err(Error::Capacity);
        }
        let mut output = vec![];
        if let Some(old) = self.active.get_mut(&identity) {
            if old.fingerprint == fingerprint {
                record.observed_at = old.row.record.observed_at;
                record.last_observed_at =
                    record.last_observed_at.max(old.row.record.last_observed_at);
                let row = QueryRecord::new(old.row.key.clone(), record)?;
                if row.record.last_observed_at != old.row.record.last_observed_at
                    || row.record.valid_until != old.row.record.valid_until
                {
                    output.push(row.clone());
                }
                old.row = row;
                self.bytes = self.bytes.saturating_sub(old.bytes).saturating_add(bytes);
                old.bytes = bytes;
                return Ok(output);
            }
            old.row.record.closed_at = Some(at);
            output.push(old.row.clone());
        } else if self.active.len() >= 20000 {
            return Err(Error::Capacity);
        }
        let key = digest(&(&identity, &fingerprint, at))?;
        let row = QueryRecord::new(key, record)?;
        self.bytes = self
            .bytes
            .saturating_sub(previous_bytes)
            .saturating_add(bytes);
        self.active.insert(
            identity,
            Version {
                fingerprint,
                row: row.clone(),
                bytes,
            },
        );
        output.push(row);
        Ok(output)
    }
    /// Recovery updates the retained triggering record before it disappears from current state.
    pub fn close(
        &mut self,
        finding: &str,
        at: chrono::DateTime<chrono::Utc>,
        removed: bool,
    ) -> Option<QueryRecord> {
        let mut version = self.active.remove(&format!("Finding/{finding}"))?;
        self.bytes = self.bytes.saturating_sub(version.bytes);
        version.row.record.closed_at = Some(at);
        if let Details::Finding { state, .. } = &mut version.row.record.details {
            *state = if removed {
                FindingState::Removed
            } else {
                FindingState::Recovered
            };
        }
        Some(version.row)
    }
    /// Release state can disappear through complete inventory removal; avoid retaining it forever.
    pub fn retain(&mut self, resources: &std::collections::BTreeSet<&str>) {
        self.active.retain(|key, value| {
            key.starts_with("Finding/")
                || value
                    .row
                    .record
                    .resource
                    .as_deref()
                    .is_some_and(|id| resources.contains(id))
        });
        self.bytes = self.active.values().map(|v| v.bytes).sum();
    }
}

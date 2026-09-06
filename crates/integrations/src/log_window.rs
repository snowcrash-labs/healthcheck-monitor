//! Bounded diagnostic windows with overlap deduplication before signature grouping.
use crate::{
    logs::Groups,
    projection::{observation, operation},
    transport::Error,
};
use chrono::{DateTime, Duration, Utc};
use monitor_core::{
    config::{duration::Span, resolve::Job},
    model::*,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
pub struct Window<'a> {
    id: String,
    dedupe: Option<&'a crate::log_dedup::Dedupe>,
    committed: Option<DateTime<Utc>>,
    gap_seconds: u64,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub scanned: usize,
    limit: usize,
    duplicates: usize,
    fingerprints: BTreeSet<[u8; 32]>,
    groups: Groups,
    contexts: BTreeMap<String, monitor_core::diagnostics::ResourceContext>,
}
impl<'a> Window<'a> {
    pub fn new(
        job: &Job,
        id: &str,
        span: Span,
        limit: usize,
        dedupe: Option<&'a crate::log_dedup::Dedupe>,
    ) -> Self {
        let end = job.log_end.unwrap_or_else(Utc::now);
        let full = end - Duration::seconds(span.0 as i64);
        let start = job.log_start.map_or(full, |at| {
            full.max(at - Duration::seconds(job.settings.log_overlap.0 as i64))
        });
        Self {
            committed: job.log_start,
            gap_seconds: job
                .log_start
                .map_or(0, |at| (full - at).num_seconds().max(0) as u64),
            id: id.into(),
            dedupe,
            start,
            end,
            scanned: 0,
            limit,
            duplicates: 0,
            fingerprints: BTreeSet::new(),
            groups: Groups::default(),
            contexts: BTreeMap::new(),
        }
    }
    /// Raw payloads and provider IDs are hashed only for transient deduplication and then discarded.
    pub fn record(
        &mut self,
        scope: &str,
        id: Option<&str>,
        message: &str,
        at: DateTime<Utc>,
    ) -> Result<(), Error> {
        if self.scanned >= self.limit {
            return Err(Error::Limit);
        }
        self.scanned += 1;
        if at < self.start || at > self.end {
            return Err(Error::Malformed);
        }
        let mut hash = Sha256::new();
        hash.update(self.id.as_bytes());
        for part in [scope.as_bytes(), id.unwrap_or(message).as_bytes()] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part);
        }
        hash.update(at.timestamp_nanos_opt().unwrap_or_default().to_be_bytes());
        let fingerprint = hash.finalize().into();
        if !self.fingerprints.insert(fingerprint)
            || self
                .dedupe
                .is_some_and(|dedupe| !dedupe.accept(fingerprint, self.end, self.committed))
        {
            self.duplicates += 1;
            return Ok(());
        }
        self.groups.add_for(scope, message, at);
        Ok(())
    }
    /// Retain only common location fields when a diagnostic group spans multiple resources.
    pub fn record_context(
        &mut self,
        scope: &str,
        id: Option<&str>,
        message: &str,
        at: DateTime<Utc>,
        context: Option<monitor_core::diagnostics::ResourceContext>,
    ) -> Result<(), Error> {
        self.record(scope, id, message, at)?;
        if let Some(context) = context {
            let scope = crate::projection::identity(scope);
            self.contexts
                .entry(scope.clone())
                .and_modify(|old| {
                    if old.native_id != context.native_id {
                        old.native_id = scope.clone();
                    }
                    if old.name != context.name {
                        old.name = None;
                    }
                    if old.uid != context.uid {
                        old.uid = None;
                    }
                    if old.container != context.container {
                        old.container = None;
                    }
                    if old.cluster != context.cluster {
                        old.cluster = None;
                    }
                    if old.namespace != context.namespace {
                        old.namespace = None;
                    }
                    if old.region != context.region {
                        old.region = None;
                    }
                    if old.zone != context.zone {
                        old.zone = None;
                    }
                })
                .or_insert(context);
        }
        Ok(())
    }
    pub fn finish(
        self,
        job: &Job,
        id: &str,
        result: &mut CheckResult,
        mut outcome: Result<(), Error>,
        pages: usize,
    ) {
        if job.continuous && self.dedupe.is_none() && outcome.is_ok() {
            outcome = Err(Error::Limit);
        }
        for (signature, mut data) in self.groups.finish_scoped() {
            if let Data::Log { sampled, .. } = &mut data {
                *sampled = outcome.is_err();
            }
            let mut observation = observation(job, id, &signature, data);
            if let Some((scope, _)) = signature.rsplit_once('/') {
                observation.context = self.contexts.get(scope).cloned().or(observation.context);
            }
            result.observations.push(observation);
        }
        let mut window = observation(
            job,
            id,
            "window",
            Data::LogWindow {
                gap_seconds: self.gap_seconds,
                start: self.start,
                end: self.end,
                scanned: self.scanned,
                duplicates: self.duplicates,
                limit: self.limit,
                complete: outcome.is_ok(),
            },
        );
        window.observed_at = self.end;
        result.observations.push(window);
        if self.gap_seconds > 0 {
            result.operations.push(operation(
                &format!("{id}/missing-window"),
                Err(&Error::Missing),
                0,
                job.settings.required,
            ));
        }
        result.operations.push(operation(
            id,
            outcome.map(|()| self.scanned).as_ref().copied(),
            pages,
            job.settings.required,
        ));
    }
}

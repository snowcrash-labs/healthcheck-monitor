//! Shared runtime ceilings use the tightest limit among selected checks.
use super::resolve::Job;
use crate::error::Error;
pub fn normalize(jobs: &mut [Job]) -> Result<(), Error> {
    let mut footprint = Footprint::default();
    for job in jobs.iter() {
        footprint.admit(&job.target, &job.settings, &[], &job.severity)?;
    }
    let Some(first) = jobs.first() else {
        return Ok(());
    };
    let mut shared = first.settings.clone();
    let mut scopes = std::collections::BTreeMap::new();
    for job in jobs.iter() {
        let settings = &job.settings;
        shared.concurrency = shared.concurrency.min(settings.concurrency);
        shared.subprocesses = shared.subprocesses.min(settings.subprocesses);
        shared.memory_bytes = shared.memory_bytes.min(settings.memory_bytes);
        shared.max_assets = shared.max_assets.min(settings.max_assets);
        shared.max_findings = shared.max_findings.min(settings.max_findings);
        shared.history_interval = shared.history_interval.min(settings.history_interval);
        shared.history_age = shared.history_age.min(settings.history_age);
        shared.history_count = shared.history_count.min(settings.history_count);
        shared.history_bytes = shared.history_bytes.min(settings.history_bytes);
        let limit = scopes
            .entry(job.scope())
            .or_insert(settings.scope_concurrency);
        *limit = (*limit).min(settings.scope_concurrency);
    }
    for job in jobs.iter_mut() {
        let scope_limit = scopes.get(&job.scope()).copied().unwrap_or(1);
        let settings = &mut job.settings;
        settings.concurrency = shared.concurrency;
        settings.subprocesses = shared.subprocesses;
        settings.memory_bytes = shared.memory_bytes;
        settings.max_assets = shared.max_assets;
        settings.max_findings = shared.max_findings;
        settings.max_series = settings.max_series.min(shared.max_assets);
        settings.history_interval = shared.history_interval;
        settings.history_age = shared.history_age;
        settings.history_count = shared.history_count;
        settings.history_bytes = shared.history_bytes;
        settings.scope_concurrency = scope_limit.min(shared.concurrency);
        settings.validate().map_err(|_| {
            Error::Config("selected checks exceed their shared runtime limits".into())
        })?;
    }
    Ok(())
}
/// Account for scheduler, reload-channel and comparison copies before cloning large job settings.
pub(super) struct Footprint {
    used: usize,
    limit: usize,
}
impl Default for Footprint {
    fn default() -> Self {
        Self {
            used: 0,
            limit: usize::MAX,
        }
    }
}
impl Footprint {
    pub fn admit(
        &mut self,
        target: &super::types::Target,
        settings: &super::settings::Settings,
        selectors: &[String],
        severity: &std::collections::BTreeMap<String, crate::model::Severity>,
    ) -> Result<(), Error> {
        let mut bytes = serde_json::to_vec(target)?.len();
        if !selectors.is_empty() {
            bytes = bytes
                .saturating_sub(serde_json::to_vec(&target.resources)?.len())
                .saturating_add(serde_json::to_vec(selectors)?.len());
        }
        bytes = bytes
            .saturating_add(serde_json::to_vec(severity)?.len())
            .saturating_add(4096)
            .saturating_mul(4);
        self.limit = self.limit.min(settings.memory_bytes / 8);
        self.used = self.used.saturating_add(bytes);
        if self.used > self.limit {
            return Err(Error::Config(
                "effective configuration exceeds the shared memory budget".into(),
            ));
        }
        Ok(())
    }
}

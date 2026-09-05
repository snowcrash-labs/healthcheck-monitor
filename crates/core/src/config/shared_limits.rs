//! Shared runtime ceilings use the tightest limit among selected checks.
use super::resolve::Job;
use crate::error::Error;
pub fn normalize(jobs: &mut [Job]) -> Result<(), Error> {
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

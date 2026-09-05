//! Reject unsafe identifiers, contradictory limits, and ambiguous selections.
use super::{types::Config, settings::Settings};
use crate::error::Error;
use std::collections::BTreeSet;
impl Settings {
    pub fn validate(&self) -> Result<(), Error> {
        let invalid = self.concurrency == 0 || self.concurrency > 256
            || self.scope_concurrency == 0 || self.scope_concurrency > self.concurrency
            || self.subprocesses == 0 || self.subprocesses > self.concurrency
            || self.attempts == 0 || self.attempts > 10
            || self.samples == 0 || self.samples > 100
            || self.connect_timeout > self.attempt_timeout || self.attempt_timeout > self.operation_timeout
            || self.interval.0 == 0 || self.sample_interval.0 == 0
            || self.max_pages == 0 || self.max_pages > 1000 || self.page_size == 0 || self.page_size > 1000
            || self.max_assets == 0 || self.max_assets > 1_000_000 || self.max_findings == 0
            || self.max_findings > self.max_assets || self.ready_queue < self.concurrency || self.ready_queue > 65536
            || self.response_bytes < 1024 || self.response_bytes > 16 * 1024 * 1024
            || self.response_bytes.saturating_mul(self.concurrency).saturating_mul(4) > self.memory_bytes
            || self.max_assets.saturating_mul(1024) > self.memory_bytes
            || self.memory_bytes > 4 * 1024 * 1024 * 1024_usize
            || self.history_count == 0 || self.history_count > 10000 || self.history_age < self.history_interval
            || self.history_bytes < self.response_bytes as u64 || self.history_bytes > 100 * 1024 * 1024 * 1024
            || self.max_series == 0 || self.max_series > self.max_assets
            || self.log_entries == 0 || self.log_entries > 50000 || self.runtime_entries == 0 || self.runtime_entries > 50000
            || !self.capacity_warning.is_finite() || !self.capacity_error.is_finite()
            || self.capacity_warning <= 0.0 || self.capacity_warning >= self.capacity_error || self.capacity_error > 100.0
            || self.recover_confirmations == 0 || self.removal_confirmations == 0;
        if invalid { return Err(Error::Config("inconsistent or excessive collection/retention limits".into())); }
        Ok(())
    }
}
/// Identifiers cannot inject query syntax, path traversal, or terminal control codes.
pub fn identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && !value.contains("..")
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./:@".contains(&b))
}
impl Config {
    pub fn validate(&self) -> Result<(), Error> {
        if self.version != 1 || self.targets.is_empty() || self.targets.len() > 128 {
            return Err(Error::Config("version must be 1 with 1..128 explicit targets".into()));
        }
        let mut names = BTreeSet::new();
        for target in &self.targets {
            if !identifier(&target.name) || !identifier(&target.scope) || !names.insert(&target.name) {
                return Err(Error::Config("target names must be unique and scope identifiers valid".into()));
            }
            if let Some(name) = &target.credential {
                let profile = self.credentials.get(name).ok_or_else(|| Error::Config("unknown credential profile".into()))?;
                if profile.provider != target.provider { return Err(Error::Config("credential provider differs from target".into())); }
            }
            for value in target.regions.iter().chain(&target.resources).chain(&target.repositories).chain(&target.watched_secrets).chain(target.context.iter()) {
                if !identifier(value) { return Err(Error::Config("invalid resource selector".into())); }
            }
            if target.regions.len() > 32 || target.metrics.len() > 500 || target.endpoints.len() > 256 { return Err(Error::Config("target exceeds collection bounds".into())); }
            for endpoint in &target.endpoints {
                if endpoint.url.scheme() != "https" || endpoint.url.host_str().is_none() || !endpoint.url.username().is_empty() || endpoint.url.password().is_some() || endpoint.url.query().is_some() || endpoint.url.fragment().is_some() || endpoint.accepted.is_empty() || endpoint.accepted.iter().any(|s| !(100..=599).contains(s)) {
                    return Err(Error::Config("endpoint needs an HTTPS URL without credentials/query and explicit accepted statuses".into()));
                }
            }
            if let Some(url) = &target.nats_url {
                if !matches!(url.scheme(), "tls" | "nats") || !url.username().is_empty() || url.password().is_some() {
                    return Err(Error::Config("NATS URL must not contain credentials".into()));
                }
            }
            for metric in &target.metrics {
                if !identifier(&metric.name) || !identifier(&metric.namespace) || !identifier(&metric.metric) || !identifier(&metric.resource) || metric.dimensions.len() > 30 {
                    return Err(Error::Config("invalid metric identity".into()));
                }
                if [metric.capacity, metric.warning, metric.error].into_iter().flatten().any(|v| !v.is_finite() || v < 0.0) || metric.capacity == Some(0.0) {
                    return Err(Error::Config("invalid metric threshold or capacity".into()));
                }
            }
        }
        for root in &self.discovery {
            if !identifier(&root.scope) { return Err(Error::Config("invalid discovery scope".into())); }
        }
        Ok(())
    }
}


//! Relationships and hard ceilings keep collection and retention bounded.
use super::settings::Settings;
use crate::error::Error;
impl Settings {
    pub fn validate(&self) -> Result<(), Error> {
        let invalid = self.jitter_percent > 50
            || self
                .slo_burn_rate_error
                .is_some_and(|value| !value.is_finite() || value <= 0.0)
            || self.log_dedup_entries == 0
            || self.log_dedup_entries > 200000
            || self.log_overlap > self.log_window
            || self.log_overlap > self.runtime_window
            || self.concurrency == 0
            || self.concurrency > 256
            || self.scope_concurrency == 0
            || self.scope_concurrency > self.concurrency
            || self.subprocesses == 0
            || self.subprocesses > self.concurrency
            || self.attempts == 0
            || self.attempts > 10
            || self.samples == 0
            || self.samples > 100
            || self.connect_timeout > self.attempt_timeout
            || self.attempt_timeout > self.operation_timeout
            || self.interval.0 == 0
            || self.sample_interval.0 == 0
            || self.max_pages == 0
            || self.max_pages > 1000
            || self.page_size == 0
            || self.page_size > 1000
            || self.max_assets == 0
            || self.max_assets > 1_000_000
            || self.max_findings == 0
            || self.max_findings > self.max_assets
            || self.ready_queue < self.concurrency
            || self.ready_queue > 65536
            || self.response_bytes < 1024
            || self.response_bytes > 16 * 1024 * 1024
            || self
                .response_bytes
                .saturating_mul(self.concurrency)
                .saturating_mul(4)
                > self.memory_bytes
            || self.max_assets.saturating_mul(1024) > self.memory_bytes
            || self.memory_bytes > 4 * 1024 * 1024 * 1024_usize
            || self.history_count == 0
            || self.history_count > 10000
            || self.history_age < self.history_interval
            || self.history_bytes < self.response_bytes as u64
            || self.history_bytes > 100 * 1024 * 1024 * 1024
            || self.max_series == 0
            || self.max_series > self.max_assets
            || self.log_entries == 0
            || self.log_entries > 50000
            || self.runtime_entries == 0
            || self.runtime_entries > 50000
            || !self.capacity_warning.is_finite()
            || !self.capacity_error.is_finite()
            || self.capacity_warning <= 0.0
            || self.capacity_warning >= self.capacity_error
            || self.capacity_error > 100.0
            || self.recover_confirmations == 0
            || self.removal_confirmations == 0;
        if invalid {
            return Err(Error::Config(
                "inconsistent or excessive collection/retention limits".into(),
            ));
        }
        Ok(())
    }
}

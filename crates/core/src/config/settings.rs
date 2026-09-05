//! Defaults and sparse overlays are kept distinct to preserve precedence.
use super::duration::Span;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub interval: Span,
    pub samples: u32,
    pub sample_interval: Span,
    pub connect_timeout: Span,
    pub attempt_timeout: Span,
    pub operation_timeout: Span,
    pub attempts: usize,
    pub concurrency: usize,
    pub scope_concurrency: usize,
    pub subprocesses: usize,
    pub ready_queue: usize,
    pub jitter_percent: u8,
    pub max_assets: usize,
    pub max_findings: usize,
    pub memory_bytes: usize,
    pub response_bytes: usize,
    pub max_pages: usize,
    pub page_size: usize,
    pub max_series: usize,
    pub log_window: Span,
    pub log_entries: usize,
    pub runtime_window: Span,
    pub runtime_entries: usize,
    pub metric_window: Span,
    pub history_interval: Span,
    pub history_age: Span,
    pub history_count: usize,
    pub history_bytes: u64,
    pub node_grace: Span,
    pub rollout_grace: Span,
    pub capacity_warning: f64,
    pub capacity_error: f64,
    pub capacity_sustain: Span,
    pub certificate_warning: Span,
    pub recovery_age_error: Option<Span>,
    pub recover_confirmations: u32,
    pub removal_confirmations: u32,
    pub stale_after: Option<Span>,
    pub queue_age_error: Option<f64>,
    pub latency_error_ms: Option<f64>,
    pub enabled: bool,
    pub required: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            interval: Span(300),
            samples: 5,
            sample_interval: Span(30),
            connect_timeout: Span(10),
            attempt_timeout: Span(30),
            operation_timeout: Span(90),
            attempts: 3,
            concurrency: 16,
            scope_concurrency: 4,
            subprocesses: 2,
            ready_queue: 1024,
            jitter_percent: 10,
            max_assets: 50_000,
            max_findings: 10_000,
            memory_bytes: 256 * 1024 * 1024,
            response_bytes: 2 * 1024 * 1024,
            max_pages: 50,
            page_size: 100,
            max_series: 1000,
            log_window: Span(3600),
            log_entries: 500,
            runtime_window: Span(86400),
            runtime_entries: 250,
            metric_window: Span(900),
            history_interval: Span(300),
            history_age: Span(86400),
            history_count: 288,
            history_bytes: 1024 * 1024 * 1024,
            node_grace: Span(300),
            rollout_grace: Span(600),
            capacity_warning: 80.0,
            capacity_error: 90.0,
            capacity_sustain: Span(600),
            certificate_warning: Span(14 * 86400),
            recovery_age_error: None,
            recover_confirmations: 2,
            removal_confirmations: 2,
            stale_after: None,
            queue_age_error: None,
            latency_error_ms: None,
            enabled: true,
            required: true,
        }
    }
}
pub use super::patch::SettingsPatch;
impl Settings {
    /// Apply only explicitly provided values.
    pub fn overlay(&mut self, patch: &SettingsPatch) {
        if let Some(value) = patch.certificate_warning {
            self.certificate_warning = value;
        }
        if let Some(value) = patch.recovery_age_error {
            self.recovery_age_error = value;
        }
        if let Some(value) = patch.jitter_percent {
            self.jitter_percent = value;
        }
        if let Some(value) = patch.interval {
            self.interval = value;
        }
        if let Some(value) = patch.samples {
            self.samples = value;
        }
        if let Some(value) = patch.sample_interval {
            self.sample_interval = value;
        }
        if let Some(value) = patch.connect_timeout {
            self.connect_timeout = value;
        }
        if let Some(value) = patch.attempt_timeout {
            self.attempt_timeout = value;
        }
        if let Some(value) = patch.operation_timeout {
            self.operation_timeout = value;
        }
        if let Some(value) = patch.attempts {
            self.attempts = value;
        }
        if let Some(value) = patch.concurrency {
            self.concurrency = value;
        }
        if let Some(value) = patch.scope_concurrency {
            self.scope_concurrency = value;
        }
        if let Some(value) = patch.subprocesses {
            self.subprocesses = value;
        }
        if let Some(value) = patch.ready_queue {
            self.ready_queue = value;
        }
        if let Some(value) = patch.max_assets {
            self.max_assets = value;
        }
        if let Some(value) = patch.max_findings {
            self.max_findings = value;
        }
        if let Some(value) = patch.memory_bytes {
            self.memory_bytes = value;
        }
        if let Some(value) = patch.response_bytes {
            self.response_bytes = value;
        }
        if let Some(value) = patch.max_pages {
            self.max_pages = value;
        }
        if let Some(value) = patch.page_size {
            self.page_size = value;
        }
        if let Some(value) = patch.max_series {
            self.max_series = value;
        }
        if let Some(value) = patch.log_window {
            self.log_window = value;
        }
        if let Some(value) = patch.log_entries {
            self.log_entries = value;
        }
        if let Some(value) = patch.runtime_window {
            self.runtime_window = value;
        }
        if let Some(value) = patch.runtime_entries {
            self.runtime_entries = value;
        }
        if let Some(value) = patch.metric_window {
            self.metric_window = value;
        }
        if let Some(value) = patch.history_interval {
            self.history_interval = value;
        }
        if let Some(value) = patch.history_age {
            self.history_age = value;
        }
        if let Some(value) = patch.history_count {
            self.history_count = value;
        }
        if let Some(value) = patch.history_bytes {
            self.history_bytes = value;
        }
        if let Some(value) = patch.node_grace {
            self.node_grace = value;
        }
        if let Some(value) = patch.rollout_grace {
            self.rollout_grace = value;
        }
        if let Some(value) = patch.capacity_warning {
            self.capacity_warning = value;
        }
        if let Some(value) = patch.capacity_error {
            self.capacity_error = value;
        }
        if let Some(value) = patch.capacity_sustain {
            self.capacity_sustain = value;
        }
        if let Some(value) = patch.recover_confirmations {
            self.recover_confirmations = value;
        }
        if let Some(value) = patch.removal_confirmations {
            self.removal_confirmations = value;
        }
        if let Some(value) = patch.stale_after {
            self.stale_after = value;
        }
        if let Some(value) = patch.queue_age_error {
            self.queue_age_error = value;
        }
        if let Some(value) = patch.latency_error_ms {
            self.latency_error_ms = value;
        }
        if let Some(value) = patch.enabled {
            self.enabled = value;
        }
        if let Some(value) = patch.required {
            self.required = value;
        }
    }
    pub fn freshness(&self) -> u64 {
        self.stale_after.map_or(
            self.interval
                .0
                .saturating_mul(2)
                .saturating_add(self.operation_timeout.0),
            |v| v.0,
        )
    }
}

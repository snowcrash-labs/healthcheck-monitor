//! Compact diagnostic projections contain metadata and measurements, never provider payloads.
use crate::enums::*;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub target: String,
    pub provider: Provider,
    pub scope: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub native_id: Option<String>,
    pub region: Option<String>,
    pub zone: Option<String>,
    pub cluster: Option<String>,
    pub namespace: Option<String>,
    pub service: Option<String>,
    pub hostname: Option<String>,
    pub uid: Option<String>,
    pub container: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fact {
    pub label: String,
    pub value: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub label: String,
    pub url: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Details {
    Finding {
        rule: String,
        severity: Severity,
        state: FindingState,
        first_detected_at: Option<DateTime<Utc>>,
        expected: String,
        confidence: String,
        facts: Vec<Fact>,
        links: Vec<Link>,
        legacy: bool,
    },
    Diagnostic {
        signature: String,
        count: u64,
        first_seen: DateTime<Utc>,
        last_seen: DateTime<Utc>,
        window_start: DateTime<Utc>,
        window_end: DateTime<Utc>,
        sampled: bool,
        scanned: u64,
        duplicates: u64,
        complete: bool,
        gap_seconds: u64,
        links: Vec<Link>,
    },
    Check {
        started_at: DateTime<Utc>,
        finished_at: DateTime<Utc>,
        oldest_observation_at: Option<DateTime<Utc>>,
        complete: bool,
        observations: u64,
        interval_seconds: u64,
        required_failures: u64,
        pending_observations: u64,
        operations: Vec<Operation>,
        operations_truncated: bool,
    },
    Release {
        revision: Option<String>,
        observed_digests: Vec<String>,
        desired_digest: Option<String>,
        pending: bool,
        verified: bool,
        facts: Vec<Fact>,
        links: Vec<Link>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub name: String,
    pub coverage: String,
    pub required: bool,
    pub observed_at: DateTime<Utc>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// Database UUIDv7 for persisted rows; a stable source identifier for current-only results.
    pub id: String,
    pub identity: String,
    pub scope: Scope,
    pub location: Location,
    pub check: Option<Check>,
    pub resource: Option<String>,
    pub observed_at: DateTime<Utc>,
    pub last_observed_at: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
    pub closed_at: Option<DateTime<Utc>>,
    pub stale: bool,
    pub details: Details,
}
impl Record {
    pub fn category(&self) -> Category {
        match self.details {
            Details::Finding { .. } => Category::Finding,
            Details::Diagnostic { .. } => Category::Diagnostic,
            Details::Check { .. } => Category::Check,
            Details::Release { .. } => Category::Release,
        }
    }
    pub fn severity(&self) -> Option<Severity> {
        match self.details {
            Details::Finding { severity, .. } => Some(severity),
            _ => None,
        }
    }
    pub fn state(&self) -> Option<FindingState> {
        match self.details {
            Details::Finding { state, .. } => Some(state),
            _ => None,
        }
    }
}

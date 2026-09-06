//! Compact read models expose observations without sharing mutable evaluation state.
use chrono::{DateTime, Utc};
use monitor_core::model::{
    Check, Confidence, Coverage, Expected, Finding, Health, Provider, Severity,
};
use serde::Serialize;
use std::collections::BTreeMap;
#[derive(Clone, Serialize)]
pub struct Fact {
    pub label: String,
    pub value: String,
}
#[derive(Clone, Serialize)]
pub struct Resource {
    pub id: String,
    pub target: String,
    pub check: Check,
    pub health: Health,
    pub expected: Expected,
    pub observed_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub facts: Vec<Fact>,
    #[serde(skip)]
    pub search: String,
}
#[derive(Clone, Serialize)]
pub struct FindingView {
    pub id: String,
    pub target: String,
    pub resource: String,
    pub rule: String,
    pub severity: Severity,
    pub observed_at: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
    pub expected: Expected,
    pub confidence: Confidence,
    pub stale: bool,
    pub evidence: Vec<String>,
}
#[derive(Clone, Serialize)]
pub struct CheckView {
    pub key: String,
    pub target: String,
    pub check: Check,
    pub interval_seconds: u64,
    pub finished_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub complete: bool,
    pub observations: usize,
    pub failures: Vec<Failure>,
}
#[derive(Clone, Serialize)]
pub struct Failure {
    pub operation: String,
    pub coverage: Coverage,
    pub required: bool,
}
#[derive(Clone, Serialize)]
pub struct Target {
    pub name: String,
    pub provider: Provider,
    pub scope: String,
    pub regions: Vec<String>,
}
pub struct View {
    pub generation: u64,
    pub revision: String,
    pub captured_at: DateTime<Utc>,
    pub targets: Vec<Target>,
    pub checks: Vec<CheckView>,
    pub resources: Vec<Resource>,
    pub findings: Vec<FindingView>,
    pub result_stamps: BTreeMap<String, ResultStamp>,
    pub persistence_fault: bool,
    pub dropped_transitions: u64,
}
#[derive(PartialEq, Eq)]
pub struct ResultStamp {
    pub revision: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
}
impl ResultStamp {
    pub fn matches(&self, result: &monitor_core::model::CheckResult) -> bool {
        self.revision == result.revision
            && self.started_at == result.started_at
            && self.finished_at == result.finished_at
    }
}
impl FindingView {
    pub fn source(&self) -> Finding {
        Finding {
            check: None,
            id: self.id.clone(),
            resource: self.resource.clone(),
            rule: self.rule.clone(),
            severity: self.severity,
            evidence: self.evidence.clone(),
            observed_at: self.observed_at,
            expected: self.expected,
            confidence: self.confidence,
            stale: self.stale,
            clear_count: 0,
            valid_until: self.valid_until,
        }
    }
}
pub fn current_health(health: Health, expires: DateTime<Utc>, now: DateTime<Utc>) -> Health {
    if now > expires {
        Health::Unknown
    } else {
        health
    }
}

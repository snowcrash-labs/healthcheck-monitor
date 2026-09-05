//! Versioned allowlisted evidence; arbitrary JSON has no place in this model.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Gcp,
    Aws,
    Azure,
    Kubernetes,
    Github,
    Edge,
    Nats,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    Preflight,
    Discovery,
    Inventory,
    Kubernetes,
    Edge,
    Managed,
    Queues,
    Releases,
    Github,
    Metrics,
    Logs,
    Alerts,
    Slo,
    Flows,
}
impl Check {
    pub const ALL: [Self; 14] = [
        Self::Preflight,
        Self::Discovery,
        Self::Inventory,
        Self::Kubernetes,
        Self::Edge,
        Self::Managed,
        Self::Queues,
        Self::Releases,
        Self::Github,
        Self::Metrics,
        Self::Logs,
        Self::Alerts,
        Self::Slo,
        Self::Flows,
    ];
    pub fn interval_seconds(self) -> u64 {
        match self {
            Self::Preflight | Self::Kubernetes | Self::Edge | Self::Queues | Self::Flows => 30,
            Self::Inventory | Self::Managed => 900,
            Self::Discovery => 3600,
            _ => 300,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Expected {
    #[default]
    Active,
    Dormant,
    ScaleToZero,
    Suspended,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
    ExpectedInactive,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    Complete,
    Denied,
    Unauthenticated,
    Unavailable,
    Unsupported,
    Missing,
    Truncated,
    Timeout,
    Cancelled,
    Malformed,
    Stale,
    InventoryOnly,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub id: String,
    pub coverage: Coverage,
    pub observed_at: DateTime<Utc>,
    pub records: usize,
    pub pages: usize,
    pub attempts: usize,
    pub required: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub resource: String,
    pub operation: String,
    pub observed_at: DateTime<Utc>,
    pub expected: Expected,
    pub data: Data,
}
pub use crate::observations::{Data, LogClass, ServiceState};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    #[serde(default)]
    pub check: Option<Check>,
    pub id: String,
    pub resource: String,
    pub rule: String,
    pub severity: Severity,
    pub evidence: Vec<String>,
    pub observed_at: DateTime<Utc>,
    pub expected: Expected,
    pub confidence: Confidence,
    pub stale: bool,
    pub clear_count: u32,
    #[serde(default)]
    pub valid_until: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Direct,
    Correlated,
    Insufficient,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub target: String,
    pub check: Check,
    pub revision: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub operations: Vec<Operation>,
    pub observations: Vec<Observation>,
}
impl CheckResult {
    pub fn complete(&self) -> bool {
        !self.operations.is_empty()
            && self
                .operations
                .iter()
                .all(|o| !o.required || o.coverage == Coverage::Complete)
    }
    pub fn failure(target: String, check: Check, revision: String, coverage: Coverage) -> Self {
        let now = Utc::now();
        Self {
            target,
            check,
            revision,
            started_at: now,
            finished_at: now,
            operations: vec![Operation {
                id: format!("{check:?}"),
                coverage,
                observed_at: now,
                records: 0,
                pages: 0,
                attempts: 1,
                required: true,
            }],
            observations: vec![],
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub revision: String,
    pub captured_at: DateTime<Utc>,
    pub selected_scope: Vec<String>,
    #[serde(default)]
    pub samples: BTreeMap<String, u32>,
    #[serde(default)]
    pub selectors: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub freshness: BTreeMap<String, u64>,
    pub results: BTreeMap<String, CheckResult>,
    pub findings: BTreeMap<String, Finding>,
    #[serde(default)]
    pub health: BTreeMap<String, Health>,
    #[serde(default)]
    pub progress: BTreeMap<String, crate::flows::Progress>,
    #[serde(default)]
    pub retired: BTreeMap<String, Transition>,
    pub confirmations: BTreeMap<String, RemovalConfirmation>,
    pub persistence_fault: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    New,
    Worsened,
    Recovered,
    Stale,
    Removed,
    Reappeared,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub at: DateTime<Utc>,
    pub finding: String,
    pub kind: TransitionKind,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemovalConfirmation {
    pub count: u32,
    pub last_at: DateTime<Utc>,
}

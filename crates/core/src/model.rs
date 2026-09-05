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
}
impl Check {
    pub const ALL: [Self; 13] = [
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
    ];
    pub fn interval_seconds(self) -> u64 {
        match self {
            Self::Preflight | Self::Kubernetes | Self::Edge | Self::Queues => 30,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Data {
    Endpoint {
        dns: bool,
        tls: bool,
        status: Option<u16>,
        accepted: Vec<u16>,
        latency_ms: u64,
        expires_at: Option<DateTime<Utc>>,
    },
    Job {
        complete: bool,
        failed: bool,
        failed_attempts: u32,
        succeeded: u32,
        active: u32,
        created_at: Option<DateTime<Utc>>,
    },
    Workload {
        desired: u32,
        ready: u32,
        created_at: Option<DateTime<Utc>>,
        draining: bool,
        node: bool,
    },
    Pod {
        uid: String,
        container: String,
        ready: bool,
        restarts: u32,
        crash_loop: bool,
        created_at: Option<DateTime<Utc>>,
        terminated_at: Option<DateTime<Utc>>,
    },
    Schedule {
        schedule: String,
        timezone: String,
        suspended: bool,
        active: u32,
        last_schedule: Option<DateTime<Utc>>,
        last_success: Option<DateTime<Utc>>,
    },
    Queue {
        backlog: f64,
        activation: f64,
        ready: u32,
        desired: u32,
        crash_loop: bool,
        scaler_ready: bool,
        age_seconds: Option<f64>,
        dead_letters: Option<f64>,
    },
    Service {
        state: ServiceState,
        replicas: Option<u32>,
        backup_enabled: Option<bool>,
        encrypted: Option<bool>,
    },
    Metric {
        name: String,
        value: f64,
        capacity: Option<f64>,
        warning: Option<f64>,
        error: Option<f64>,
        window_seconds: u64,
    },
    Build {
        pipeline: String,
        revision: String,
        target: String,
        state: ServiceState,
        created_at: DateTime<Utc>,
    },
    Image {
        desired: String,
        observed_digest: Option<String>,
        revision: Option<String>,
    },
    Log {
        signature: LogClass,
        count: u64,
        first_seen: DateTime<Utc>,
        last_seen: DateTime<Utc>,
        sampled: bool,
    },
    Inventory {
        family: String,
        supported: bool,
    },
    Condition {
        rule: String,
        healthy: Option<bool>,
    },
    Identity {
        scope: String,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Ready,
    Starting,
    Stopped,
    Failed,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogClass {
    Import,
    Panic,
    OutOfMemory,
    Connection,
    Permission,
    Timeout,
    Warning,
    OtherError,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
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
    pub results: BTreeMap<String, CheckResult>,
    pub findings: BTreeMap<String, Finding>,
    pub confirmations: BTreeMap<String, u32>,
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

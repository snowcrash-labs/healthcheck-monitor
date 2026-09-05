//! Allowlisted observation variants; provider payloads are discarded at the boundary.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Data {
    Quota { region: String, code: String, limit: f64, usage: Option<crate::config::types::MetricQuery> },
    Progress {
        state: crate::model::Health,
    },
    Scaler {
        namespace: String,
        name: String,
        worker: String,
        metric: String,
        activation: f64,
        ready: bool,
    },
    Owner {
        uid: String,
        owner_uid: Option<String>,
    },
    AdvertisedEndpoint {
        url: String,
    },
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
        superseded: bool,
        pipeline: String,
        revision: String,
        target: String,
        state: ServiceState,
        created_at: Option<DateTime<Utc>>,
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

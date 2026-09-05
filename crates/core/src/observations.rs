//! Allowlisted observation variants; provider payloads are discarded at the boundary.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Data {
    Registry {
        host: String,
    },
    Artifact {
        manifest: ManifestKind,
        children: Vec<String>,
        image: String,
        digest: String,
        tags: Vec<String>,
        revision: Option<String>,
        repository: Option<String>,
        built: bool,
        created_at: Option<DateTime<Utc>>,
    },
    Commit {
        repository: String,
        revision: String,
        reference: Option<String>,
    },
    Provenance {
        valid_until: DateTime<Utc>,
        observed_digests: Vec<String>,
        desired_digest: Option<String>,
        pending: bool,
        revision: Option<String>,
        repository: Option<String>,
        registry_verified: bool,
        build_verified: bool,
        commit_verified: bool,
        mismatch: bool,
    },
    SloDefinition {
        name: String,
        goal: Option<f64>,
        period_seconds: Option<u64>,
    },
    Slo {
        goal: f64,
        compliance: Option<f64>,
        budget: Option<f64>,
        burn_rate: Option<f64>,
        period_seconds: Option<u64>,
    },
    LogWorkspace {
        workspace_id: String,
        resource_id: String,
    },
    LogWindow {
        gap_seconds: u64,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        scanned: usize,
        duplicates: usize,
        limit: usize,
        complete: bool,
    },
    Certificate {
        issued: Option<bool>,
        expires_at: Option<DateTime<Utc>>,
    },
    Recovery {
        last_attempt: Option<DateTime<Utc>>,
        state: ServiceState,
        enabled: Option<bool>,
        last_success: Option<DateTime<Utc>>,
        retention_days: Option<u32>,
        point_in_time: Option<bool>,
        geo_redundant: Option<bool>,
    },
    MetricResource {
        resource_id: String,
        namespace: String,
    },
    Quota {
        region: String,
        code: String,
        limit: f64,
        usage: Option<crate::config::types::MetricQuery>,
    },
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
        #[serde(default)]
        completed_at: Option<DateTime<Utc>>,
        #[serde(default)]
        scheduled_at: Option<DateTime<Utc>>,
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
        #[serde(default)]
        created_at: Option<DateTime<Utc>>,
        #[serde(default)]
        starting_deadline_seconds: Option<u64>,
        #[serde(default)]
        forbid_overlap: bool,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestKind {
    Image,
    Index,
    Unknown,
}
impl Data {
    /// Inventory and collection metadata do not imply resource health.
    pub fn is_health_evidence(&self) -> bool {
        !matches!(
            self,
            Self::Registry { .. }
                | Self::Artifact { .. }
                | Self::Commit { .. }
                | Self::SloDefinition { .. }
                | Self::LogWorkspace { .. }
                | Self::LogWindow { .. }
                | Self::MetricResource { .. }
                | Self::Quota { .. }
                | Self::Owner { .. }
                | Self::Inventory { .. }
                | Self::Identity { .. }
                | Self::Scaler { .. }
                | Self::AdvertisedEndpoint { .. }
        )
    }
}

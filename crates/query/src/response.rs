//! Every historical answer declares its evidence boundary and missing coverage.
use crate::{
    enums::*,
    filter::Window,
    record::{Record, Scope},
};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Availability {
    pub requested: Window,
    pub available_since: Option<DateTime<Utc>>,
    pub persisted_through: Option<DateTime<Utc>>,
    pub history_available: bool,
    pub complete: bool,
    pub gaps: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub availability: Availability,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScopeInfo {
    pub scope: Scope,
    pub checks: Vec<Check>,
    pub current: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Summary {
    pub availability: Availability,
    pub findings: u64,
    pub errors: u64,
    pub warnings: u64,
    pub recovered: u64,
    pub failed_checks: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Assessment {
    pub outcome: Outcome,
    pub window: Window,
    pub baseline: Window,
    pub availability: Availability,
    pub reasons: Vec<String>,
    pub outstanding_checks: Vec<String>,
    pub next_observation_at: Option<DateTime<Utc>>,
    pub new_findings: Vec<Record>,
    pub worsened_findings: Vec<Record>,
    pub pre_existing_findings: Vec<Record>,
    pub recovered_findings: Vec<Record>,
    pub next_cursor: Option<String>,
}

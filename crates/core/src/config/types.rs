//! Versioned operator configuration; secrets are referenced, never embedded.
use super::settings::SettingsPatch;
use crate::model::{Check, Expected, Provider, Severity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub settings: SettingsPatch,
    #[serde(default)]
    pub credentials: BTreeMap<String, Credential>,
    #[serde(default)]
    pub discovery: Vec<DiscoveryRoot>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
    #[serde(default)]
    pub checks: BTreeMap<Check, SettingsPatch>,
    #[serde(default)]
    pub severity: BTreeMap<String, Severity>,
    pub targets: Vec<Target>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credential {
    pub provider: Provider,
    pub profile: Option<String>,
    pub tenant: Option<String>,
    pub role_arn: Option<String>,
    pub expected_identity: Option<String>,
    pub token_env: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryRoot {
    pub provider: Provider,
    pub scope: String,
    pub credential: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub checks: Option<Vec<Check>>,
    #[serde(default)]
    pub settings: SettingsPatch,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub name: String,
    pub provider: Provider,
    pub scope: String,
    pub credential: Option<String>,
    #[serde(default)]
    pub regions: Vec<String>,
    pub context: Option<String>,
    pub timezone: Option<String>,
    #[serde(default)]
    pub expected: Expected,
    #[serde(default)]
    pub expectations: BTreeMap<String, Expected>,
    #[serde(default)]
    pub resources: Vec<String>,
    #[serde(default)]
    pub settings: SettingsPatch,
    #[serde(default)]
    pub checks: BTreeMap<Check, SettingsPatch>,
    #[serde(default)]
    pub endpoints: Vec<Endpoint>,
    #[serde(default)]
    pub repositories: Vec<String>,
    pub desired_file: Option<String>,
    pub source_ref: Option<String>,
    #[serde(default)]
    pub watched_secrets: Vec<String>,
    #[serde(default)]
    pub metrics: Vec<MetricQuery>,
    #[serde(default)]
    pub slo_goals: BTreeMap<String, f64>,
    pub nats_url: Option<Url>,
    pub nats_fallback: Option<NatsFallback>,
    #[serde(default)]
    pub flows: Vec<Flow>,
    pub change: Option<ChangeScope>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeScope {
    pub repository: String,
    pub base: String,
    pub head: String,
    pub paths: Vec<String>,
    #[serde(default)]
    pub workloads: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    pub name: String,
    pub demand: String,
    pub idle_after: super::duration::Span,
    pub stages: Vec<FlowStage>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowStage {
    pub name: String,
    pub progress: String,
    pub mode: SignalMode,
    pub workload: Option<String>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalMode {
    Counter,
    Rate,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub name: String,
    pub url: Url,
    pub accepted: Vec<u16>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricQuery {
    #[serde(default)]
    pub aggregation: Aggregation,
    pub name: String,
    pub namespace: String,
    pub metric: String,
    pub resource: String,
    #[serde(default)]
    pub dimensions: BTreeMap<String, String>,
    pub capacity: Option<f64>,
    pub warning: Option<f64>,
    pub error: Option<f64>,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aggregation {
    #[default]
    Minimum,
    Maximum,
    Average,
    Sum,
    Latest,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NatsFallback {
    pub namespace: String,
    pub deployment: String,
}

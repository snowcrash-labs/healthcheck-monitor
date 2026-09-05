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
pub struct DiscoveryRoot { pub provider: Provider, pub scope: String, pub credential: Option<String> }
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
    #[serde(default)]
    pub expected: Expected,
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
    pub nats_url: Option<Url>,
    pub nats_fallback: Option<NatsFallback>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint { pub name: String, pub url: Url, pub accepted: Vec<u16> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricQuery {
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NatsFallback { pub namespace: String, pub deployment: String }


//! Operator-owned source configuration never accepts caller SQL or arbitrary download URLs.
use crate::{error::Error, model::Provider};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub reader_access: Option<ReaderAccess>,
    pub sources: Vec<Source>,
    pub scope_targets: Vec<ScopeTarget>,
    pub interval_seconds: Option<u64>,
    pub backfill_days: Option<u16>,
    pub retention_days: Option<u16>,
    pub max_rows: Option<usize>,
    pub max_bytes_billed: Option<u64>,
    pub daily_bytes_billed: Option<u64>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderAccess {
    DashboardReaders,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeTarget {
    pub provider: Provider,
    pub scope: String,
    pub target: String,
    pub valid_from: Option<chrono::NaiveDate>,
    pub valid_to: Option<chrono::NaiveDate>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub provider: Provider,
    pub billing_scope: String,
    pub gcp: Option<Gcp>,
    pub aws_query: Option<AwsQuery>,
    pub azure_query: Option<AzureQuery>,
    pub credential: Option<monitor_core::config::types::Credential>,
}
/// Cost Explorer is an explicit aggregate bootstrap option with no resource attribution.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AwsQuery {
    pub region: String,
}
/// Azure's read-only cost query adapter is used when an export is not configured.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AzureQuery {
    pub api_version: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gcp {
    pub project: String,
    pub dataset: String,
    pub table: String,
    pub location: String,
    #[serde(default)]
    pub detailed: bool,
}
impl Config {
    /// Ambiguous or out-of-period environment mappings remain unallocated.
    pub fn attribute(&self, charge: &mut crate::model::Charge) {
        let mut matches = self.scope_targets.iter().filter(|m| {
            m.provider == charge.provider
                && charge.scope.as_ref() == Some(&m.scope)
                && m.valid_from.is_none_or(|d| charge.day >= d)
                && m.valid_to.is_none_or(|d| charge.day < d)
        });
        charge.target = match (matches.next(), matches.next()) {
            (Some(mapping), None) => Some(mapping.target.clone()),
            _ => None,
        };
    }
    pub fn interval(&self) -> u64 {
        self.interval_seconds.unwrap_or(3600)
    }
    pub fn backfill(&self) -> u16 {
        self.backfill_days.unwrap_or(90)
    }
    pub fn retention(&self) -> u16 {
        self.retention_days.unwrap_or(400)
    }
    pub fn rows(&self) -> usize {
        self.max_rows.unwrap_or(100_000)
    }
    pub fn query_bytes(&self) -> u64 {
        self.max_bytes_billed.unwrap_or(2 * 1024 * 1024 * 1024)
    }
    pub fn daily_bytes(&self) -> u64 {
        self.daily_bytes_billed.unwrap_or(20 * 1024 * 1024 * 1024)
    }
    pub fn validate(&self) -> Result<(), Error> {
        if self.enabled && self.reader_access.is_none()
            || self.sources.len() > 16
            || self.scope_targets.len() > 128
            || !(900..=86400).contains(&self.interval())
            || !(1..=400).contains(&self.backfill())
            || !(self.backfill()..=730).contains(&self.retention())
            || !(100..=500_000).contains(&self.rows())
            || self.query_bytes() == 0
            || self.query_bytes() > self.daily_bytes()
            || self.daily_bytes() > 1024 * 1024 * 1024 * 1024
        {
            return Err(Error::Configuration);
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut scopes = std::collections::BTreeSet::new();
        for source in &self.sources {
            if let Some(credential) = &source.credential {
                credential.validate().map_err(|_| Error::Configuration)?;
                if !matches!(
                    (source.provider, credential.provider),
                    (Provider::Gcp, monitor_core::model::Provider::Gcp)
                        | (Provider::Aws, monitor_core::model::Provider::Aws)
                        | (Provider::Azure, monitor_core::model::Provider::Azure)
                ) {
                    return Err(Error::Configuration);
                }
            }
            if !identifier(&source.id)
                || source.billing_scope.is_empty()
                || source.billing_scope.len() > 128
                || !ids.insert(&source.id)
                || !scopes.insert((source.provider, &source.billing_scope))
            {
                return Err(Error::Configuration);
            }
            let adapters = usize::from(source.gcp.is_some())
                + usize::from(source.aws_query.is_some())
                + usize::from(source.azure_query.is_some());
            if adapters != 1 {
                return Err(Error::Configuration);
            }
            match (&source.gcp, source.provider) {
                (Some(gcp), Provider::Gcp)
                    if [&gcp.project, &gcp.dataset, &gcp.table, &gcp.location]
                        .iter()
                        .all(|v| identifier(v)) => {}
                (None, Provider::Aws)
                    if source
                        .aws_query
                        .as_ref()
                        .is_some_and(|a| identifier(&a.region))
                        && source.billing_scope.len() == 12
                        && source.billing_scope.bytes().all(|b| b.is_ascii_digit()) => {}
                (None, Provider::Azure)
                    if source
                        .azure_query
                        .as_ref()
                        .is_some_and(|a| identifier(&a.api_version))
                        && source.billing_scope.len() == 36
                        && source
                            .billing_scope
                            .bytes()
                            .all(|b| b.is_ascii_hexdigit() || b == b'-') => {}
                _ => return Err(Error::Configuration),
            }
        }
        for mapping in &self.scope_targets {
            if mapping.scope.is_empty() || mapping.scope.len() > 128 || !identifier(&mapping.target)
            {
                return Err(Error::Configuration);
            }
        }
        Ok(())
    }
}
pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

//! Validate and resolve the entire replacement before installing schedules.
use super::{
    settings::{Settings, SettingsPatch},
    types::{Config, Profile, Target},
};
use crate::{error::Error, model::Check};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub profile: Option<String>,
    pub targets: Vec<String>,
    pub checks: Vec<Check>,
    pub resources: Vec<String>,
    pub overrides: SettingsPatch,
}
#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub key: String,
    pub target: Target,
    pub check: Check,
    pub settings: Settings,
    pub revision: String,
    pub flows_enabled: bool,
    pub flow_settings: Option<Settings>,
    pub severity: std::collections::BTreeMap<String, crate::model::Severity>,
}
impl Job {
    pub fn scope(&self) -> String {
        format!("{:?}/{}", self.target.provider, self.target.scope)
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Effective {
    pub revision: String,
    pub jobs: Vec<Job>,
}
impl Config {
    /// Parse and validate the new format without importing historical evidence.
    pub fn parse(text: &str) -> Result<Self, Error> {
        if text.len() > 1024 * 1024 {
            return Err(Error::Config("configuration exceeds 1 MiB".into()));
        }
        let mut config: Self = toml::from_str(text)
            .map_err(|_| Error::Config("invalid TOML or unknown fields".into()))?;
        for (name, checks, samples, entries, window) in [
            (
                "quick",
                vec![
                    Check::Preflight,
                    Check::Edge,
                    Check::Kubernetes,
                    Check::Queues,
                ],
                1,
                500,
                3600,
            ),
            ("full", Check::ALL.to_vec(), 5, 500, 3600),
            ("deep", Check::ALL.to_vec(), 5, 5000, 86400),
        ] {
            config
                .profiles
                .entry(name.into())
                .or_insert_with(|| Profile {
                    checks: Some(checks),
                    settings: SettingsPatch {
                        samples: (name == "quick").then_some(samples),
                        log_entries: (name == "deep").then_some(entries),
                        log_window: (name == "deep").then_some(super::duration::Span(window)),
                        ..Default::default()
                    },
                });
        }
        config.validate()?;
        Ok(config)
    }
    /// Defaults, global, profile, target, global check, target check, CLI.
    pub fn resolve(&self, selection: &Selection) -> Result<Effective, Error> {
        let profile_name = selection.profile.as_deref().unwrap_or("full");
        let profile = self
            .profiles
            .get(profile_name)
            .ok_or_else(|| Error::Config("unknown profile".into()))?;
        for name in &selection.targets {
            if !self.targets.iter().any(|t| &t.name == name) {
                return Err(Error::Config("unknown target".into()));
            }
        }
        let encoded = serde_json::to_vec(&(
            self,
            profile_name,
            &selection.targets,
            &selection.checks,
            &selection.resources,
            &selection.overrides,
        ))?;
        let revision: String = Sha256::digest(encoded)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let mut jobs = Vec::new();
        for target in self
            .targets
            .iter()
            .filter(|t| selection.targets.is_empty() || selection.targets.contains(&t.name))
        {
            let selected = if selection.checks.is_empty() {
                profile
                    .checks
                    .clone()
                    .unwrap_or_else(|| Check::ALL.to_vec())
            } else {
                selection.checks.clone()
            };
            let mut checks: BTreeSet<Check> = selected
                .into_iter()
                .filter(|c| applicable(target, *c))
                .collect();
            checks.insert(Check::Preflight);
            if checks.iter().any(|c| {
                matches!(
                    c,
                    Check::Managed | Check::Releases | Check::Queues | Check::Edge
                )
            }) && target.provider != crate::model::Provider::Edge
            {
                if applicable(target, Check::Inventory) {
                    checks.insert(Check::Inventory);
                }
                if target.context.is_some() {
                    checks.insert(Check::Kubernetes);
                }
            }
            if checks.contains(&Check::Flows) {
                checks.insert(Check::Metrics);
                if target.context.is_some() {
                    checks.insert(Check::Kubernetes);
                    checks.insert(Check::Queues);
                }
            }
            let flows_enabled = checks.contains(&Check::Flows);
            for check in checks {
                let mut settings = Settings {
                    interval: super::duration::Span(check.interval_seconds()),
                    samples: if check == Check::Queues { 5 } else { 1 },
                    ..Default::default()
                };
                settings.overlay(&self.settings);
                settings.overlay(&profile.settings);
                settings.overlay(&target.settings);
                if let Some(patch) = self.checks.get(&check) {
                    settings.overlay(patch);
                }
                if let Some(patch) = target.checks.get(&check) {
                    settings.overlay(patch);
                }
                settings.overlay(&selection.overrides);
                settings.validate()?;
                if !settings.enabled {
                    continue;
                }
                let mut target = target.clone();
                if !selection.resources.is_empty() {
                    target.resources = selection.resources.clone();
                }
                jobs.push(Job {
                    key: format!("{}/{check:?}", target.name),
                    target,
                    check,
                    settings,
                    revision: revision.clone(),
                    flows_enabled,
                    flow_settings: None,
                    severity: self.severity.clone(),
                });
            }
        }
        let flow_settings: std::collections::BTreeMap<_, _> = jobs
            .iter()
            .filter(|job| job.check == Check::Flows)
            .map(|job| (job.target.name.clone(), job.settings.clone()))
            .collect();
        for job in &mut jobs {
            job.flow_settings = flow_settings.get(&job.target.name).cloned();
            job.flows_enabled = job.flow_settings.is_some();
        }
        if jobs.is_empty() || jobs.len() > 4096 {
            return Err(Error::Config("select between 1 and 4096 checks".into()));
        }
        Ok(Effective { revision, jobs })
    }
}
/// Select meaningful checks while preserving explicit unsupported provider outcomes.
pub fn applicable(target: &Target, check: Check) -> bool {
    use crate::model::Provider;
    match check {
        Check::Flows => !target.flows.is_empty(),
        Check::Preflight => true,
        Check::Kubernetes => target.context.is_some() || target.provider == Provider::Kubernetes,
        Check::Edge => {
            !target.endpoints.is_empty()
                || matches!(
                    target.provider,
                    Provider::Gcp
                        | Provider::Aws
                        | Provider::Azure
                        | Provider::Kubernetes
                        | Provider::Edge
                )
        }
        Check::Github => target.provider == Provider::Github || !target.repositories.is_empty(),
        _ => match target.provider {
            Provider::Edge => false,
            Provider::Github => {
                matches!(check, Check::Discovery | Check::Inventory | Check::Releases)
            }
            Provider::Kubernetes => matches!(
                check,
                Check::Inventory | Check::Queues | Check::Releases | Check::Managed
            ),
            Provider::Nats => check == Check::Queues,
            _ => true,
        },
    }
}

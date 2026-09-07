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
    pub artifact_only: bool,
    pub assess_health: bool,
    pub requested_checks: BTreeSet<Check>,
    pub kube_only: bool,
    #[serde(skip)]
    pub sampling_explicit: bool,
    #[serde(skip)]
    pub interval_explicit: bool,
    #[serde(skip)]
    pub continuous: bool,
    #[serde(skip)]
    pub log_start: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip)]
    pub log_end: Option<chrono::DateTime<chrono::Utc>>,
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
    pub fn observation_scope(&self) -> String {
        let mut regions = self.target.regions.clone();
        regions.sort_unstable();
        regions.dedup();
        format!(
            "{:?}",
            (
                self.target.provider,
                &self.target.scope,
                &self.target.context,
                &self.target.cluster,
                &self.target.cluster_location,
                &self.target.credential,
                regions
            )
        )
    }
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
        self.resolve_inner(selection, false)
    }
    pub(super) fn resolve_inner(
        &self,
        selection: &Selection,
        metadata_only: bool,
    ) -> Result<Effective, Error> {
        if selection.resources.len() > 1024
            || selection
                .resources
                .iter()
                .any(|selector| !super::validate::identifier(selector))
        {
            return Err(Error::Config(
                "invalid or excessive CLI resource selectors".into(),
            ));
        }
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
        let mut footprint = super::shared_limits::Footprint::default();
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
            let kube_only = target.context.is_some()
                && checks.iter().any(|check| *check != Check::Preflight)
                && checks.iter().all(|check| {
                    matches!(check, Check::Preflight | Check::Kubernetes | Check::Queues)
                });
            let requested_checks = checks.clone();
            if target.provider != crate::model::Provider::Github
                && !selection.checks.contains(&Check::Discovery)
                && !self
                    .discovery
                    .iter()
                    .any(|root| root.provider == target.provider)
            {
                checks.remove(&Check::Discovery);
            }
            checks.insert(Check::Preflight);
            if !metadata_only
                && checks
                    .iter()
                    .any(|c| matches!(c, Check::Managed | Check::Releases))
                && target.provider != crate::model::Provider::Edge
            {
                if applicable(target, Check::Inventory) {
                    checks.insert(Check::Inventory);
                }
                if target.context.is_some() && checks.contains(&Check::Releases) {
                    checks.insert(Check::Kubernetes);
                }
            }
            if target.context.is_some()
                && checks
                    .iter()
                    .any(|check| matches!(check, Check::Queues | Check::Edge))
            {
                checks.insert(Check::Kubernetes);
            }
            if checks.contains(&Check::Flows) && !target.flows.is_empty() {
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
                    samples: if check == Check::Queues
                        || check == Check::Flows && !target.flows.is_empty()
                    {
                        5
                    } else {
                        1
                    },
                    ..Default::default()
                };
                let empty = SettingsPatch::default();
                let patches = [
                    &self.settings,
                    &profile.settings,
                    &target.settings,
                    self.checks.get(&check).unwrap_or(&empty),
                    target.checks.get(&check).unwrap_or(&empty),
                    &selection.overrides,
                ];
                let sampling_explicit = patches.iter().any(|patch| patch.samples.is_some());
                let interval_explicit = patches.iter().any(|patch| patch.interval.is_some());
                for patch in patches {
                    settings.overlay(patch);
                }
                settings.validate()?;
                if !settings.enabled {
                    continue;
                }
                footprint.admit(target, &settings, &selection.resources, &self.severity)?;
                let mut target = target.clone();
                if !selection.resources.is_empty() {
                    target.resources = selection.resources.clone();
                }
                jobs.push(Job {
                    artifact_only: metadata_only,
                    assess_health: requested_checks.contains(&check) || check == Check::Preflight,
                    requested_checks: requested_checks.clone(),
                    kube_only,
                    sampling_explicit,
                    interval_explicit,
                    continuous: false,
                    log_start: None,
                    log_end: None,
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
        super::prerequisites::sampling(&mut jobs)?;
        self.reference_jobs(selection, &mut jobs, &revision, metadata_only)?;
        super::shared_limits::normalize(&mut jobs)?;
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
pub use super::applicability::applicable;

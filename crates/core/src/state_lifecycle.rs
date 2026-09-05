//! Freshness and selection lifecycle for bounded comparison state.
use crate::{config::resolve::Job, model::*, state::State};
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
impl State {
    pub(crate) fn apply_derived(
        &mut self,
        job: &Job,
        result: CheckResult,
        now: DateTime<Utc>,
    ) -> Vec<Transition> {
        let samples = self.snapshot.samples.get(&job.key).copied();
        let transitions = self.apply(job, result, now);
        match samples {
            Some(samples) => {
                self.snapshot.samples.insert(job.key.clone(), samples);
            }
            None => {
                self.snapshot.samples.remove(&job.key);
            }
        }
        transitions
    }
    /// Reference facts can arrive after runtime inventories during a one-off collection.
    pub fn refresh_releases(&mut self, jobs: &[Job], now: DateTime<Utc>) -> Vec<Transition> {
        let mut transitions = Vec::new();
        for job in jobs
            .iter()
            .filter(|job| job.check == Check::Releases && job.target.provider != Provider::Github)
        {
            if let Some(result) = self.snapshot.results.get(&job.key).cloned() {
                transitions.extend(self.apply_derived(job, result, now));
            }
        }
        transitions
    }
    pub fn new(revision: String, scope: Vec<String>) -> Self {
        Self {
            snapshot: Snapshot {
                version: 1,
                revision,
                captured_at: Utc::now(),
                selected_scope: scope,
                scope_fingerprints: BTreeMap::new(),
                samples: BTreeMap::new(),
                selectors: BTreeMap::new(),
                freshness: BTreeMap::new(),
                results: BTreeMap::new(),
                findings: BTreeMap::new(),
                health: BTreeMap::new(),
                pressure: BTreeMap::new(),
                progress: BTreeMap::new(),
                retired: BTreeMap::new(),
                confirmations: BTreeMap::new(),
                persistence_fault: false,
            },
        }
    }

    pub(crate) fn trim_retired(&mut self, limit: usize) {
        while self.snapshot.retired.len() > limit {
            let oldest = self
                .snapshot
                .retired
                .iter()
                .min_by_key(|(_, transition)| transition.at)
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                self.snapshot.retired.remove(&oldest);
            } else {
                break;
            }
        }
    }

    /// A one-off run establishes its own sampling baseline while retaining unresolved findings.
    pub fn begin_run(&mut self) {
        self.snapshot.results.clear();
        self.snapshot.health.clear();
        self.snapshot.pressure.clear();
        self.snapshot.progress.clear();
        self.snapshot.confirmations.clear();
        self.snapshot.samples.clear();
    }
    pub fn expire(&mut self, freshness: u64, now: DateTime<Utc>) -> Vec<Transition> {
        let mut transitions = Vec::new();
        for (key, result) in &mut self.snapshot.results {
            let limit = self
                .snapshot
                .freshness
                .get(key)
                .copied()
                .unwrap_or(freshness);
            for operation in &mut result.operations {
                if operation.coverage == Coverage::Complete
                    && (now - operation.observed_at).num_seconds() > limit as i64
                {
                    operation.coverage = Coverage::Stale;
                }
            }
            for observation in &result.observations {
                if ((now - observation.observed_at).num_seconds() > limit as i64
                    || matches!(observation.data,Data::Provenance{valid_until,..}if valid_until<now))
                    && self.snapshot.health.contains_key(&observation.resource)
                {
                    self.snapshot
                        .health
                        .insert(observation.resource.clone(), Health::Unknown);
                    for operation in &mut result.operations {
                        if operation.id == observation.operation
                            && operation.coverage == Coverage::Complete
                        {
                            operation.coverage = Coverage::Stale;
                        }
                    }
                }
            }
        }
        for finding in self.snapshot.findings.values_mut() {
            if !finding.stale
                && finding.valid_until.map_or(
                    (now - finding.observed_at).num_seconds() > freshness as i64,
                    |until| now > until,
                )
            {
                finding.stale = true;
                transitions.push(Transition {
                    at: now,
                    finding: finding.id.clone(),
                    kind: TransitionKind::Stale,
                });
            }
        }
        transitions
    }
    pub fn retain_scope(&mut self, jobs: &[Job]) {
        if jobs
            .first()
            .is_some_and(|job| job.revision != self.snapshot.revision)
        {
            self.snapshot.pressure.clear();
            self.snapshot.progress.clear();
        }
        for job in jobs {
            if self
                .snapshot
                .scope_fingerprints
                .get(&job.key)
                .is_none_or(|scope| scope != &job.observation_scope())
            {
                self.snapshot.results.remove(&job.key);
                self.snapshot
                    .retired
                    .retain(|id, _| !id.starts_with(&format!("{}/", job.target.name)));
                self.snapshot.findings.retain(|_, finding| {
                    !(finding
                        .resource
                        .starts_with(&format!("{}/", job.target.name))
                        && finding.check.is_none_or(|check| check == job.check))
                });
                self.snapshot
                    .pressure
                    .retain(|resource, _| !resource.starts_with(&format!("{}/", job.target.name)));
                self.snapshot
                    .progress
                    .retain(|resource, _| !resource.starts_with(&format!("{}/", job.target.name)));
            }
            if self
                .snapshot
                .selectors
                .get(&job.key)
                .is_some_and(|selectors| selectors != &job.target.resources)
            {
                self.snapshot.results.remove(&job.key);
            }
        }
        self.snapshot.selectors = jobs
            .iter()
            .map(|job| (job.key.clone(), job.target.resources.clone()))
            .collect();
        self.snapshot.scope_fingerprints = jobs
            .iter()
            .map(|job| (job.key.clone(), job.observation_scope()))
            .collect();
        let flow_keys: BTreeSet<_> = jobs
            .iter()
            .filter(|job| job.flows_enabled)
            .flat_map(|job| {
                job.target.flows.iter().flat_map(move |flow| {
                    flow.stages.iter().map(move |stage| {
                        format!("{}/flows/{}/{}", job.target.name, flow.name, stage.name)
                    })
                })
            })
            .collect();
        self.snapshot
            .progress
            .retain(|key, _| flow_keys.contains(key));
        let keys: BTreeSet<_> = jobs.iter().map(|j| &j.key).collect();
        self.snapshot.results.retain(|k, _| keys.contains(k));
        let observed: BTreeSet<_> = self
            .snapshot
            .results
            .values()
            .flat_map(|result| &result.observations)
            .map(|obs| &obs.resource)
            .collect();
        self.snapshot
            .health
            .retain(|resource, _| observed.contains(resource));
        self.snapshot.freshness.retain(|key, _| keys.contains(key));
        for job in jobs {
            self.snapshot
                .freshness
                .insert(job.key.clone(), job.settings.freshness());
        }
        self.snapshot.samples.retain(|key, _| keys.contains(key));
        self.snapshot.findings.retain(|_, finding| {
            jobs.iter().any(|job| {
                finding
                    .resource
                    .starts_with(&format!("{}/", job.target.name))
                    && finding.check.is_none_or(|check| check == job.check)
                    && (job.target.resources.is_empty()
                        || job
                            .target
                            .resources
                            .iter()
                            .any(|selector| finding.resource.contains(selector)))
            })
        });
        self.snapshot.confirmations.retain(|key, _| {
            self.snapshot
                .findings
                .contains_key(key.split_once('|').map_or(key.as_str(), |(_, id)| id))
        });
        self.snapshot.selected_scope = jobs.iter().map(|j| j.key.clone()).collect();
    }
}

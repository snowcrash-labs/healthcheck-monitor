//! Bounded current state and confirmed transitions, independent of persistence.
use crate::{config::resolve::Job, model::*, policy::evaluate};
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};

pub struct State {
    pub snapshot: Snapshot,
}
impl State {
    pub fn new(revision: String, scope: Vec<String>) -> Self {
        Self {
            snapshot: Snapshot {
                version: 1,
                revision,
                captured_at: Utc::now(),
                selected_scope: scope,
                results: BTreeMap::new(),
                findings: BTreeMap::new(),
                confirmations: BTreeMap::new(),
                persistence_fault: false,
            },
        }
    }
    /// Failed and truncated operations cannot clear findings or imply removals.
    pub fn apply(
        &mut self,
        job: &Job,
        mut result: CheckResult,
        now: DateTime<Utc>,
    ) -> Vec<Transition> {
        let mut transitions = Vec::new();
        let old = self.snapshot.results.get(&job.key);
        let mut current = BTreeSet::new();
        let mut evaluated = BTreeSet::new();
        let remaining = job.settings.max_assets.saturating_sub(
            self.snapshot
                .results
                .iter()
                .filter(|(k, _)| *k != &job.key)
                .map(|(_, r)| r.observations.len())
                .sum(),
        );
        if result.observations.len() > remaining {
            result.observations.truncate(remaining);
            for op in &mut result.operations {
                if op.coverage == Coverage::Complete {
                    op.coverage = Coverage::Truncated;
                }
            }
        }
        for observation in &result.observations {
            let prior = old.and_then(|r| {
                r.observations
                    .iter()
                    .find(|o| o.resource == observation.resource)
            });
            let evaluation = evaluate(observation, prior, &job.settings, now);
            if result
                .operations
                .iter()
                .any(|o| o.id == observation.operation && o.coverage == Coverage::Complete)
                && matches!(
                    evaluation.health,
                    Health::Healthy | Health::ExpectedInactive
                )
            {
                evaluated.insert((
                    observation.resource.clone(),
                    observation.operation.clone(),
                    observation.observed_at,
                ));
            }
            for mut finding in evaluation.findings {
                current.insert(finding.id.clone());
                let previous = self.snapshot.findings.get(&finding.id);
                if previous.is_none() && self.snapshot.findings.len() >= job.settings.max_findings {
                    for op in &mut result.operations {
                        if op.coverage == Coverage::Complete {
                            op.coverage = Coverage::Truncated;
                        }
                    }
                    continue;
                }
                let transition = match previous {
                    None => Some(TransitionKind::New),
                    Some(old) if old.stale => Some(TransitionKind::Reappeared),
                    Some(old) if finding.severity > old.severity => Some(TransitionKind::Worsened),
                    _ => None,
                };
                if let Some(kind) = transition {
                    transitions.push(Transition {
                        at: now,
                        finding: finding.id.clone(),
                        kind,
                    });
                }
                finding.clear_count = 0;
                self.snapshot.findings.insert(finding.id.clone(), finding);
            }
        }
        let old_resources: BTreeSet<_> = old
            .into_iter()
            .flat_map(|r| &r.observations)
            .map(|o| &o.resource)
            .collect();
        let new_resources: BTreeSet<_> = result.observations.iter().map(|o| &o.resource).collect();
        let mut removed = Vec::new();
        for (id, finding) in &mut self.snapshot.findings {
            if current.contains(id) {
                continue;
            }
            if let Some((_, _, at)) = evaluated.iter().find(|(resource, op, at)| {
                resource == &finding.resource
                    && finding.evidence.contains(op)
                    && *at > finding.observed_at
            }) {
                finding.clear_count = finding.clear_count.saturating_add(1);
                finding.observed_at = *at;
                if finding.clear_count >= job.settings.recover_confirmations {
                    transitions.push(Transition {
                        at: now,
                        finding: id.clone(),
                        kind: TransitionKind::Recovered,
                    });
                    removed.push(id.clone());
                }
            } else if result.complete()
                && (old_resources.contains(&finding.resource)
                    || self.snapshot.confirmations.contains_key(id))
                && !new_resources.contains(&finding.resource)
                && finding
                    .resource
                    .starts_with(&format!("{}/", job.target.name))
            {
                let count = self.snapshot.confirmations.entry(id.clone()).or_insert(0);
                *count = count.saturating_add(1);
                if *count >= job.settings.removal_confirmations {
                    transitions.push(Transition {
                        at: now,
                        finding: id.clone(),
                        kind: TransitionKind::Removed,
                    });
                    removed.push(id.clone());
                }
            }
        }
        for id in removed {
            self.snapshot.findings.remove(&id);
            self.snapshot.confirmations.remove(&id);
        }
        self.snapshot.results.insert(job.key.clone(), result);
        self.snapshot.captured_at = now;
        self.snapshot.revision = job.revision.clone();
        transitions.extend(self.expire(job.settings.freshness(), now));
        transitions
    }
    pub fn expire(&mut self, freshness: u64, now: DateTime<Utc>) -> Vec<Transition> {
        let mut transitions = Vec::new();
        for finding in self.snapshot.findings.values_mut() {
            if !finding.stale && (now - finding.observed_at).num_seconds() > freshness as i64 {
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
        let keys: BTreeSet<_> = jobs.iter().map(|j| &j.key).collect();
        self.snapshot.results.retain(|k, _| keys.contains(k));
        let resources: BTreeSet<_> = self
            .snapshot
            .results
            .values()
            .flat_map(|r| &r.observations)
            .map(|o| &o.resource)
            .collect();
        self.snapshot
            .findings
            .retain(|_, finding| resources.contains(&finding.resource));
        self.snapshot
            .confirmations
            .retain(|id, _| self.snapshot.findings.contains_key(id));
        self.snapshot.selected_scope = jobs.iter().map(|j| j.key.clone()).collect();
    }
}

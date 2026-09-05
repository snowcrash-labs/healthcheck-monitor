//! Bounded current state and confirmed transitions, independent of persistence.
use crate::{config::resolve::Job, model::*, policy::evaluate};
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;

pub struct State {
    pub snapshot: Snapshot,
}
impl State {
    /// Failed and truncated operations cannot clear findings or imply removals.
    pub fn apply(
        &mut self,
        job: &Job,
        mut result: CheckResult,
        now: DateTime<Utc>,
    ) -> Vec<Transition> {
        let samples = self.snapshot.samples.entry(job.key.clone()).or_default();
        if job.assess_health {
            self.snapshot.collection_only.remove(&job.key);
        } else {
            self.snapshot.collection_only.insert(job.key.clone());
        }
        self.snapshot
            .scope_fingerprints
            .insert(job.key.clone(), job.observation_scope());
        *samples = samples.saturating_add(1);
        if job.check == Check::Flows {
            result = crate::flows::evaluate(&mut self.snapshot, job, now);
        }
        if job.check == Check::Releases && !job.artifact_only {
            crate::releases::enrich(&self.snapshot, job, &mut result, now);
        }
        let mut transitions = Vec::new();
        crate::partial_results::retain(job, &self.snapshot, &mut result);
        crate::log_recovery::reconcile(job, &self.snapshot, &mut result);
        self.snapshot
            .freshness
            .insert(job.key.clone(), job.settings.freshness());
        let other_bytes = self
            .snapshot
            .results
            .iter()
            .filter(|(key, _)| *key != &job.key)
            .fold(0usize, |total, (_, result)| {
                total.saturating_add(crate::bounds::result_bytes(result))
            });
        crate::bounds::truncate(
            &mut result,
            (job.settings.memory_bytes / 2).saturating_sub(other_bytes),
            job.settings.max_assets,
        );
        crate::provenance::mark_retries(&mut result.observations);
        crate::schedule_policy::correlate(&mut result);
        crate::recovery::consolidate(&mut result.observations);
        crate::bounds::validate_source(&mut result, job.settings.freshness(), now);
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
                r.observations.iter().find(|o| {
                    o.resource == observation.resource
                        && r.operations
                            .iter()
                            .any(|op| op.id == o.operation && op.coverage == Coverage::Complete)
                })
            });
            let complete = result
                .operations
                .iter()
                .any(|op| op.id == observation.operation && op.coverage == Coverage::Complete);
            let assess = job.assess_health
                && (job.check != Check::Releases
                    || matches!(
                        observation.data,
                        Data::Build { .. } | Data::Provenance { .. }
                    ));
            let evaluation = if !assess {
                crate::policy::Evaluation {
                    health: Health::Unknown,
                    findings: vec![],
                }
            } else {
                crate::capacity::evaluate(
                    &mut self.snapshot.pressure,
                    observation,
                    &job.settings,
                    complete,
                    now,
                )
                .unwrap_or_else(|| evaluate(observation, prior, &job.settings, now))
            };
            let health = if !complete
                && matches!(
                    evaluation.health,
                    Health::Healthy | Health::ExpectedInactive
                ) {
                Health::Unknown
            } else {
                evaluation.health
            };
            if assess && observation.data.is_health_evidence() {
                self.snapshot
                    .health
                    .insert(observation.resource.clone(), health);
            }
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
                finding.check = Some(job.check);
                finding.valid_until = finding
                    .observed_at
                    .checked_add_signed(chrono::Duration::seconds(job.settings.freshness() as i64));
                if let Data::Provenance { valid_until, .. } = &observation.data {
                    finding.valid_until = finding.valid_until.map(|at| at.min(*valid_until));
                }
                if let Some(severity) = job.severity.get(&finding.rule) {
                    finding.severity = *severity;
                }
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
                    None if self.snapshot.retired.contains_key(&finding.id) => {
                        Some(TransitionKind::Reappeared)
                    }
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
                self.snapshot.retired.remove(&finding.id);
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
            let confirmation_key = format!("{}|{id}", job.key);
            if new_resources.contains(&finding.resource) {
                self.snapshot.confirmations.remove(&confirmation_key);
            }
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
                finding.valid_until = at
                    .checked_add_signed(chrono::Duration::seconds(job.settings.freshness() as i64));
                finding.stale = false;
                if finding.clear_count >= job.settings.recover_confirmations {
                    transitions.push(Transition {
                        at: now,
                        finding: id.clone(),
                        kind: TransitionKind::Recovered,
                    });
                    removed.push((id.clone(), TransitionKind::Recovered));
                }
            } else if matches!(job.check, Check::Inventory | Check::Kubernetes)
                && result.complete()
                && (old_resources.contains(&finding.resource)
                    || self.snapshot.confirmations.contains_key(&confirmation_key))
                && !new_resources.contains(&finding.resource)
                && finding
                    .resource
                    .starts_with(&format!("{}/", job.target.name))
            {
                let observed_at = result
                    .operations
                    .iter()
                    .filter(|operation| finding.evidence.contains(&operation.id))
                    .map(|operation| operation.observed_at)
                    .max()
                    .unwrap_or(result.started_at);
                let confirmation = self
                    .snapshot
                    .confirmations
                    .entry(confirmation_key)
                    .or_insert(RemovalConfirmation {
                        count: 0,
                        last_at: finding.observed_at,
                    });
                if observed_at > confirmation.last_at {
                    confirmation.count = confirmation.count.saturating_add(1);
                    confirmation.last_at = observed_at;
                }
                if confirmation.count >= job.settings.removal_confirmations {
                    transitions.push(Transition {
                        at: now,
                        finding: id.clone(),
                        kind: TransitionKind::Removed,
                    });
                    removed.push((id.clone(), TransitionKind::Removed));
                }
            }
        }
        for (id, kind) in removed {
            self.snapshot.findings.remove(&id);
            self.snapshot
                .confirmations
                .retain(|key, _| !key.ends_with(&format!("|{id}")));
            self.snapshot.retired.insert(
                id.clone(),
                Transition {
                    at: now,
                    finding: id,
                    kind,
                },
            );
        }
        self.trim_retired(job.settings.max_findings);
        self.snapshot.results.insert(job.key.clone(), result);
        let observed: BTreeSet<_> = self
            .snapshot
            .results
            .values()
            .flat_map(|result| &result.observations)
            .map(|o| &o.resource)
            .collect();
        self.snapshot
            .health
            .retain(|resource, _| observed.contains(resource));
        self.snapshot
            .pressure
            .retain(|resource, _| observed.contains(resource));
        self.snapshot.captured_at = now;
        self.snapshot.revision = job.revision.clone();
        transitions.extend(self.expire(job.settings.freshness(), now));
        if job.flows_enabled && job.check != Check::Flows {
            let mut flow_job = job.clone();
            if let Some(settings) = &job.flow_settings {
                flow_job.settings = settings.clone();
            }
            flow_job.key = format!("{}/Flows", job.target.name);
            flow_job.check = Check::Flows;
            flow_job.flows_enabled = false;
            let result = crate::flows::evaluate(&mut self.snapshot, &flow_job, now);
            transitions.extend(self.apply_derived(&flow_job, result, now));
        }
        transitions
    }
}

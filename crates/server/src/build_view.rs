//! Project the current bounded engine state without imposing another result limit.
use crate::view::*;
use monitor_core::{
    config::resolve::Effective,
    model::{Data, Health, Snapshot},
};
use std::collections::BTreeMap;
pub fn build(snapshot: &Snapshot, effective: &Effective, generation: u64) -> View {
    let mut targets = BTreeMap::new();
    for job in &effective.jobs {
        if targets.contains_key(&job.target.name) {
            continue;
        }
        targets
            .entry(job.target.name.clone())
            .or_insert_with(|| Target {
                name: job.target.name.clone(),
                provider: job.target.provider,
                scope: job.target.scope.clone(),
                regions: job.target.regions.clone(),
            });
    }
    let targets: Vec<_> = targets.into_values().collect();
    let mut findings = Vec::new();
    for finding in snapshot.findings.values() {
        findings.push(FindingView {
            id: finding.id.clone(),
            target: target(&finding.resource, &targets),
            resource: finding.resource.clone(),
            rule: finding.rule.clone(),
            severity: finding.severity,
            observed_at: finding.observed_at,
            valid_until: finding.valid_until,
            expected: finding.expected,
            confidence: finding.confidence,
            stale: finding.stale,
            evidence: finding.evidence.clone(),
        });
    }
    let mut resources: BTreeMap<String, Resource> = BTreeMap::new();
    let mut checks = Vec::new();
    let mut result_stamps = BTreeMap::new();
    for job in &effective.jobs {
        let result = snapshot.results.get(&job.key);
        let freshness = snapshot
            .freshness
            .get(&job.key)
            .copied()
            .unwrap_or(job.settings.freshness());
        if let Some(result) = result {
            result_stamps.insert(
                job.key.clone(),
                ResultStamp {
                    revision: result.revision.clone(),
                    started_at: result.started_at,
                    finished_at: result.finished_at,
                },
            );
        }
        let mut failures = Vec::new();
        for operation in result
            .into_iter()
            .flat_map(|result| &result.operations)
            .filter(|operation| operation.coverage != monitor_core::model::Coverage::Complete)
        {
            failures.push(Failure {
                operation: operation.id.clone(),
                coverage: operation.coverage,
                required: operation.required,
            });
        }
        checks.push(CheckView {
            key: job.key.clone(),
            target: job.target.name.clone(),
            check: job.check,
            interval_seconds: job.settings.interval.0,
            finished_at: result.map(|result| result.finished_at),
            expires_at: result
                .map(|result| result.finished_at + chrono::Duration::seconds(freshness as i64)),
            complete: result.is_some_and(|result| result.complete()),
            observations: result.map_or(0, |result| result.observations.len()),
            failures,
        });
    }
    for job in &effective.jobs {
        let freshness = snapshot
            .freshness
            .get(&job.key)
            .copied()
            .unwrap_or(job.settings.freshness());
        if let Some(result) = snapshot.results.get(&job.key) {
            for observation in &result.observations {
                if matches!(observation.data, Data::Owner { .. } | Data::Scaler { .. }) {
                    continue;
                }
                if resources
                    .get(&observation.resource)
                    .is_some_and(|old| old.observed_at >= observation.observed_at)
                {
                    continue;
                }
                let facts = crate::facts::facts(&observation.data);
                let expires_at =
                    observation.observed_at + chrono::Duration::seconds(freshness as i64);
                let expires_at = match &observation.data {
                    Data::Provenance { valid_until, .. } => expires_at.min(*valid_until),
                    _ => expires_at,
                };
                resources.insert(
                    observation.resource.clone(),
                    Resource {
                        id: observation.resource.clone(),
                        target: job.target.name.clone(),
                        check: job.check,
                        health: snapshot
                            .health
                            .get(&observation.resource)
                            .copied()
                            .unwrap_or(Health::Unknown),
                        expected: observation.expected,
                        observed_at: observation.observed_at,
                        expires_at,
                        search: std::iter::once(observation.resource.as_str())
                            .chain(
                                facts
                                    .iter()
                                    .flat_map(|fact| [fact.label.as_str(), fact.value.as_str()]),
                            )
                            .collect::<Vec<_>>()
                            .join(" ")
                            .to_lowercase(),
                        facts,
                    },
                );
            }
        }
    }
    View {
        generation,
        revision: snapshot.revision.clone(),
        captured_at: snapshot.captured_at,
        targets,
        checks,
        resources: resources.into_values().collect(),
        findings,
        result_stamps,
        persistence_fault: snapshot.persistence_fault,
        dropped_transitions: snapshot.dropped_transitions,
    }
}
pub fn target(resource: &str, targets: &[Target]) -> String {
    targets
        .iter()
        .filter(|target| {
            resource
                .strip_prefix(&target.name)
                .is_some_and(|suffix| suffix.starts_with('/'))
        })
        .max_by_key(|target| target.name.len())
        .map(|target| target.name.clone())
        .unwrap_or_else(|| "unknown".into())
}

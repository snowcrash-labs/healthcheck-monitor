//! Publish one indexed resource catalog with evidence from every contributing check.
use crate::{
    resource_evidence::{Evidence, attach},
    view::Resource,
};
use monitor_core::{
    config::resolve::Effective,
    model::{Data, Health, Snapshot},
};
use std::collections::BTreeMap;

pub fn build(snapshot: &Snapshot, effective: &Effective) -> Vec<Resource> {
    let mut resources: BTreeMap<String, Resource> = BTreeMap::new();
    for job in &effective.jobs {
        let freshness = snapshot
            .freshness
            .get(&job.key)
            .copied()
            .unwrap_or(job.settings.freshness());
        for observation in snapshot
            .results
            .get(&job.key)
            .into_iter()
            .flat_map(|r| &r.observations)
        {
            if matches!(observation.data, Data::Owner { .. } | Data::Scaler { .. }) {
                continue;
            }
            let expires_at = observation.observed_at + chrono::Duration::seconds(freshness as i64);
            let expires_at = match observation.data {
                Data::Provenance { valid_until, .. } => expires_at.min(valid_until),
                _ => expires_at,
            };
            let evidence = Evidence::new(observation, job.check, expires_at);
            let resource = resources
                .entry(observation.resource.clone())
                .or_insert_with(|| Resource {
                    id: observation.resource.clone(),
                    target: job.target.name.clone(),
                    check: job.check,
                    checks: vec![],
                    context: observation.context.clone(),
                    links: vec![],
                    evidence: std::sync::Arc::new(vec![]),
                    health: snapshot
                        .health
                        .get(&observation.resource)
                        .copied()
                        .unwrap_or(Health::Unknown),
                    expected: observation.expected,
                    observed_at: observation.observed_at,
                    expires_at,
                    facts: evidence.facts.clone(),
                    search: String::new(),
                });
            if resource.observed_at < observation.observed_at {
                resource.observed_at = observation.observed_at;
                resource.expires_at = expires_at;
                resource.facts = evidence.facts.clone();
                resource.check = job.check;
                resource.context = observation.context.clone().or(resource.context.take());
            }
            attach(resource, evidence);
        }
    }
    // A failed inventory may no longer contain a resource whose finding is still active.
    for finding in snapshot.findings.values() {
        let Some(diagnostic) = &finding.diagnostic else {
            continue;
        };
        let Some(job) = effective.jobs.iter().find(|job| {
            finding.check == Some(job.check)
                && finding
                    .resource
                    .starts_with(&format!("{}/", job.target.name))
        }) else {
            continue;
        };
        resources
            .entry(finding.resource.clone())
            .or_insert_with(|| {
                let observation = &diagnostic.observation;
                let expires_at = observation.observed_at
                    + chrono::Duration::seconds(job.settings.freshness() as i64);
                let evidence = Evidence::new(observation, job.check, expires_at);
                Resource {
                    id: finding.resource.clone(),
                    target: job.target.name.clone(),
                    check: job.check,
                    checks: vec![job.check],
                    context: observation.context.clone(),
                    links: vec![],
                    evidence: std::sync::Arc::new(vec![evidence.clone()]),
                    health: Health::Unknown,
                    expected: finding.expected,
                    observed_at: observation.observed_at,
                    expires_at,
                    facts: evidence.facts,
                    search: String::new(),
                }
            });
    }
    for resource in resources.values_mut() {
        resource.links = crate::console_links::links(resource.context.as_ref());
        resource.checks.sort_unstable();
        resource.search = std::iter::once(resource.id.as_str())
            .chain(
                resource
                    .evidence
                    .iter()
                    .flat_map(|e| e.facts.iter())
                    .flat_map(|f| [f.label.as_str(), f.value.as_str()]),
            )
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if let Some(context) = &resource.context {
            resource.search.push_str(
                &format!(
                    " {} {} {} {} {}",
                    context.scope,
                    context.native_id,
                    context.region.as_deref().unwrap_or(""),
                    context.namespace.as_deref().unwrap_or(""),
                    context.cluster.as_deref().unwrap_or("")
                )
                .to_lowercase(),
            );
        }
    }
    resources.into_values().collect()
}

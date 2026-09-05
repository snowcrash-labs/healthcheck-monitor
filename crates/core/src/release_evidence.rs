//! Fresh provenance facts and authoritative Kubernetes owner traversal.
use crate::{config::resolve::Job, model::*};
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
pub struct Evidence<'a> {
    candidates: BTreeMap<&'a str, &'a Observation>,
    expiry: BTreeMap<&'a str, DateTime<Utc>>,
    pub facts: Vec<&'a Observation>,
    pub artifacts: BTreeMap<&'a str, Vec<&'a Observation>>,
    pub commits: BTreeMap<&'a str, Vec<&'a Observation>>,
    owners: BTreeMap<&'a str, (&'a str, Option<&'a str>)>,
    parents: BTreeMap<(&'a str, &'a str), Option<&'a str>>,
}
impl<'a> Evidence<'a> {
    pub fn new(
        snapshot: &'a Snapshot,
        job: &Job,
        current: &'a CheckResult,
        now: DateTime<Utc>,
    ) -> Self {
        let inputs: Vec<_> = snapshot
            .results
            .iter()
            .filter(|(key, _)| *key != &job.key)
            .map(|(key, result)| (key.as_str(), result))
            .chain(std::iter::once((job.key.as_str(), current)))
            .collect();
        let mut latest_operations = BTreeMap::new();
        for (_, result) in &inputs {
            for operation in &result.operations {
                let key = (result.target.as_str(), operation.id.as_str());
                if latest_operations
                    .get(&key)
                    .is_none_or(|at| *at < operation.observed_at)
                {
                    latest_operations.insert(key, operation.observed_at);
                }
            }
        }
        let mut facts = Vec::new();
        let mut candidates: BTreeMap<&str, &Observation> = BTreeMap::new();
        let mut expiry = BTreeMap::new();
        for (key, result) in inputs {
            let limit = snapshot
                .freshness
                .get(key)
                .copied()
                .unwrap_or(job.settings.freshness());
            for obs in &result.observations {
                if matches!(obs.data, Data::Image { .. })
                    && candidates
                        .get(obs.resource.as_str())
                        .is_none_or(|prior| prior.observed_at <= obs.observed_at)
                {
                    candidates.insert(obs.resource.as_str(), obs);
                }
            }
            facts.extend(result.observations.iter().filter(|obs| {
                let limit = source_limit(snapshot, &result.target, obs, limit);
                obs.observed_at <= now
                    && (now - obs.observed_at).num_seconds() <= limit as i64
                    && result.operations.iter().any(|op| {
                        op.id == obs.operation
                            && op.coverage == Coverage::Complete
                            && latest_operations.get(&(result.target.as_str(), op.id.as_str()))
                                == Some(&op.observed_at)
                    })
            }));
            for obs in &result.observations {
                let limit = source_limit(snapshot, &result.target, obs, limit);
                if let Some(until) = obs
                    .observed_at
                    .checked_add_signed(chrono::Duration::seconds(limit as i64))
                {
                    expiry
                        .entry(obs.resource.as_str())
                        .and_modify(|at: &mut DateTime<Utc>| *at = (*at).max(until))
                        .or_insert(until);
                }
            }
        }
        let mut latest: BTreeMap<&str, &Observation> = BTreeMap::new();
        for fact in facts {
            if latest
                .get(fact.resource.as_str())
                .is_none_or(|old| old.observed_at <= fact.observed_at)
            {
                latest.insert(&fact.resource, fact);
            }
        }
        let facts: Vec<_> = latest.into_values().collect();
        let mut artifacts: BTreeMap<&str, Vec<&Observation>> = BTreeMap::new();
        let mut commits: BTreeMap<&str, Vec<&Observation>> = BTreeMap::new();
        for fact in &facts {
            match &fact.data {
                Data::Artifact { image, .. } => artifacts.entry(image).or_default().push(fact),
                Data::Commit { revision, .. } => commits.entry(revision).or_default().push(fact),
                _ => {}
            }
        }
        let owners: BTreeMap<_, _> = facts
            .iter()
            .filter_map(|obs| match &obs.data {
                Data::Owner { uid, owner_uid } => Some((
                    obs.resource.strip_suffix("/owner")?,
                    (uid.as_str(), owner_uid.as_deref()),
                )),
                _ => None,
            })
            .collect();
        let parents = owners
            .iter()
            .map(|(resource, (uid, parent))| {
                ((resource.split('/').next().unwrap_or(""), *uid), *parent)
            })
            .collect();
        Self {
            candidates,
            expiry,
            facts,
            owners,
            parents,
            artifacts,
            commits,
        }
    }
    pub fn image_base<'b>(&self, resource: &'b str) -> (&'b str, &'b str) {
        resource.rsplit_once("/image/").unwrap_or((resource, ""))
    }
    pub fn runtime_images(&self, image: &Observation) -> Vec<&'a Observation> {
        let (base, container) = self.image_base(&image.resource);
        let target = image.resource.split('/').next().unwrap_or("");
        let Some((uid, _)) = self.owners.get(base) else {
            return vec![];
        };
        self.facts
            .iter()
            .copied()
            .filter(|candidate| {
                if !candidate.resource.starts_with(&format!("{target}/")) {
                    return false;
                }
                if !matches!(
                    candidate.data,
                    Data::Image {
                        observed_digest: Some(_),
                        ..
                    }
                ) {
                    return false;
                }
                let (candidate_base, candidate_container) = self.image_base(&candidate.resource);
                if candidate_container != container {
                    return false;
                }
                let Some((_, owner)) = self.owners.get(candidate_base) else {
                    return false;
                };
                let mut current = *owner;
                for _ in 0..64 {
                    let Some(parent) = current else {
                        return false;
                    };
                    if parent == *uid {
                        return true;
                    }
                    current = self.parents.get(&(target, parent)).copied().flatten();
                }
                false
            })
            .collect()
    }
    pub fn inactive(&self, image: &Observation) -> bool {
        if image.expected != Expected::Active {
            return true;
        }
        let (base, _) = self.image_base(&image.resource);
        self.facts.iter().any(|obs| {
            obs.resource == base
                && matches!(
                    obs.data,
                    Data::Workload { desired: 0, .. }
                        | Data::Schedule { active: 0, .. }
                        | Data::Job { complete: true, .. }
                )
        })
    }
    pub fn grace(&self, image: &Observation, job: &Job, now: DateTime<Utc>) -> bool {
        let (base, _) = self.image_base(&image.resource);
        self.facts.iter().any(|obs| {
            let at = match obs.data {
                Data::Workload { created_at, .. } if obs.resource == base => created_at,
                Data::Pod { created_at, .. } if obs.resource.starts_with(&format!("{base}/")) => {
                    created_at
                }
                _ => None,
            };
            at.is_some_and(|at| {
                (now - at).num_seconds() >= 0
                    && (now - at).num_seconds() < job.settings.rollout_grace.0 as i64
            })
        })
    }
    pub fn images(&self, target: &str) -> Vec<&'a Observation> {
        let mut seen = BTreeSet::new();
        self.candidates
            .values()
            .copied()
            .filter(|obs| {
                obs.resource.starts_with(&format!("{target}/"))
                    && matches!(obs.data, Data::Image { .. })
                    && seen.insert(obs.resource.clone())
            })
            .collect()
    }
    pub fn fresh(&self, obs: &Observation) -> bool {
        self.facts
            .iter()
            .any(|fact| fact.resource == obs.resource && fact.observed_at == obs.observed_at)
    }
    pub fn expires(&self, obs: &Observation) -> DateTime<Utc> {
        self.expiry
            .get(obs.resource.as_str())
            .copied()
            .unwrap_or(obs.observed_at)
    }
}
fn source_limit(snapshot: &Snapshot, target: &str, obs: &Observation, limit: u64) -> u64 {
    if matches!(
        obs.operation.as_str(),
        "pods"
            | "nodes"
            | "deployments"
            | "statefulsets"
            | "replicasets"
            | "daemonsets"
            | "jobs"
            | "cronjobs"
            | "scaledobjects"
    ) {
        snapshot
            .freshness
            .get(&format!("{target}/Kubernetes"))
            .copied()
            .unwrap_or(limit.min(150))
    } else {
        limit
    }
}

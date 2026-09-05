//! Indexed comparison avoids quadratic scans and requires authoritative collection for recovery.
use crate::model::*;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
pub struct Index<'a> {
    prior: BTreeMap<&'a str, &'a Observation>,
    complete: BTreeSet<String>,
    clear: BTreeMap<String, (String, DateTime<Utc>)>,
}
impl<'a> Index<'a> {
    pub fn new(old: Option<&'a CheckResult>, result: &CheckResult) -> Self {
        let old_complete: BTreeSet<_> = old
            .into_iter()
            .flat_map(|result| &result.operations)
            .filter(|operation| operation.coverage == Coverage::Complete)
            .map(|operation| operation.id.as_str())
            .collect();
        Self {
            prior: old
                .into_iter()
                .flat_map(|result| &result.observations)
                .filter(|observation| old_complete.contains(observation.operation.as_str()))
                .map(|observation| (observation.resource.as_str(), observation))
                .collect(),
            complete: complete(result),
            clear: BTreeMap::new(),
        }
    }
    pub fn prior(&self, observation: &Observation) -> Option<&'a Observation> {
        self.prior.get(observation.resource.as_str()).copied()
    }
    pub fn complete(&self, observation: &Observation) -> bool {
        self.complete.contains(&observation.operation)
    }
    pub fn record_clear(&mut self, observation: &Observation, health: Health) {
        if self.complete(observation)
            && matches!(health, Health::Healthy | Health::ExpectedInactive)
        {
            self.clear.insert(
                observation.resource.clone(),
                (observation.operation.clone(), observation.observed_at),
            );
        }
    }
    /// An evaluation limit reached later in the scan invalidates earlier clear decisions too.
    pub fn reconcile(&mut self, result: &CheckResult, health: &mut BTreeMap<String, Health>) {
        self.complete = complete(result);
        self.clear
            .retain(|_, (operation, _)| self.complete.contains(operation));
        for observation in &result.observations {
            if !self.complete(observation)
                && let Some(state) = health.get_mut(&observation.resource)
                && matches!(*state, Health::Healthy | Health::ExpectedInactive)
            {
                *state = Health::Unknown;
            }
        }
    }
    pub fn clear(&self, finding: &Finding) -> Option<DateTime<Utc>> {
        self.clear
            .get(&finding.resource)
            .filter(|(operation, at)| {
                finding.evidence.contains(operation) && *at > finding.observed_at
            })
            .map(|(_, at)| *at)
    }
    pub fn can_remove(&self, finding: &Finding) -> bool {
        !finding.evidence.is_empty()
            && finding
                .evidence
                .iter()
                .all(|operation| self.complete.contains(operation))
    }
}
fn complete(result: &CheckResult) -> BTreeSet<String> {
    result
        .operations
        .iter()
        .filter(|operation| operation.coverage == Coverage::Complete)
        .map(|operation| operation.id.clone())
        .collect()
}

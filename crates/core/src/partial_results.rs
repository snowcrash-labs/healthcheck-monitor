//! Failed refreshes retain earlier evidence without granting it complete current coverage.
use crate::{config::resolve::Job, model::*};
use std::collections::BTreeSet;
pub fn retain(job: &Job, snapshot: &Snapshot, result: &mut CheckResult) {
    if result
        .operations
        .iter()
        .all(|operation| operation.coverage == Coverage::Complete)
    {
        return;
    }
    let Some(previous) = snapshot.results.get(&job.key) else {
        return;
    };
    let resources: BTreeSet<_> = result
        .observations
        .iter()
        .map(|obs| obs.resource.clone())
        .collect();
    let complete: BTreeSet<_> = result
        .operations
        .iter()
        .filter(|op| op.coverage == Coverage::Complete)
        .map(|op| op.id.clone())
        .collect();
    let mut retained = BTreeSet::new();
    let mut available =
        (job.settings.memory_bytes / 4).saturating_sub(crate::bounds::result_bytes(result));
    for observation in &previous.observations {
        if result.observations.len() >= job.settings.max_assets {
            break;
        }
        if !resources.contains(&observation.resource) && !complete.contains(&observation.operation)
        {
            let bytes = crate::bounds::observation_bytes(observation).saturating_add(1024);
            if bytes > available {
                break;
            }
            available -= bytes;
            retained.insert(observation.operation.clone());
            result.observations.push(observation.clone());
        }
    }
    for operation in &previous.operations {
        if retained.contains(&operation.id)
            && !result.operations.iter().any(|op| op.id == operation.id)
        {
            let mut operation = operation.clone();
            operation.coverage = Coverage::Stale;
            result.operations.push(operation);
        }
    }
}

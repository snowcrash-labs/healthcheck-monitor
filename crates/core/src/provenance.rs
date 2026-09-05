//! Retry correlation requires the same pipeline, revision, target, and provider scope.
use crate::model::{Data, Observation, ServiceState};
use std::collections::BTreeMap;
pub fn mark_retries(observations: &mut [Observation]) {
    let mut success = BTreeMap::new();
    for observation in observations.iter() {
        if let Data::Build {
            pipeline,
            revision,
            target,
            state: ServiceState::Ready,
            created_at,
            ..
        } = &observation.data
        {
            let Some(created_at) = created_at else {
                continue;
            };
            if pipeline.is_empty() || revision.is_empty() || target.is_empty() {
                continue;
            }
            let scope = scope(observation);
            success
                .entry((scope, pipeline.clone(), revision.clone(), target.clone()))
                .and_modify(|time: &mut chrono::DateTime<chrono::Utc>| {
                    *time = (*time).max(*created_at)
                })
                .or_insert(*created_at);
        }
    }
    for observation in observations.iter_mut() {
        let scope = scope(observation);
        if let Data::Build {
            pipeline,
            revision,
            target,
            state: ServiceState::Failed,
            created_at,
            superseded,
        } = &mut observation.data
        {
            let Some(created_at) = created_at else {
                continue;
            };
            *superseded = success
                .get(&(scope, pipeline.clone(), revision.clone(), target.clone()))
                .is_some_and(|time| time > created_at);
        }
    }
}
fn scope(observation: &Observation) -> String {
    let target = observation.resource.split('/').next().unwrap_or("");
    let mut parts = observation.operation.split('/');
    let family = parts.next().unwrap_or("");
    let context = if family == "workflows" {
        parts.take(2).collect::<Vec<_>>().join("/")
    } else {
        parts.next().unwrap_or("global").to_owned()
    };
    format!("{target}/{context}")
}

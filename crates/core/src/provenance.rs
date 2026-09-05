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
            let scope = observation.resource.split('/').next().unwrap_or("");
            success
                .entry((
                    scope.to_string(),
                    pipeline.clone(),
                    revision.clone(),
                    target.clone(),
                ))
                .and_modify(|time: &mut chrono::DateTime<chrono::Utc>| {
                    *time = (*time).max(*created_at)
                })
                .or_insert(*created_at);
        }
    }
    for observation in observations.iter_mut() {
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
            let scope = observation.resource.split('/').next().unwrap_or("");
            *superseded = success
                .get(&(
                    scope.to_string(),
                    pipeline.clone(),
                    revision.clone(),
                    target.clone(),
                ))
                .is_some_and(|time| time > created_at);
        }
    }
}

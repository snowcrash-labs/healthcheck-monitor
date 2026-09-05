//! Consolidate recovery points per resource, preserving the latest attempt and latest success.
use crate::model::*;
use std::collections::BTreeMap;
pub fn consolidate(observations: &mut Vec<Observation>) {
    let mut indices: BTreeMap<String, usize> = BTreeMap::new();
    let mut output: Vec<Observation> = Vec::with_capacity(observations.len());
    for obs in observations.drain(..) {
        if let Data::Recovery {
            last_attempt,
            last_success,
            ..
        } = &obs.data
        {
            if let Some(index) = indices.get(&obs.resource).copied() {
                let old = &mut output[index];
                if let Data::Recovery {
                    last_attempt: previous,
                    last_success: success,
                    ..
                } = &mut old.data
                {
                    let combined = (*success).max(*last_success);
                    if last_attempt > previous {
                        *old = obs;
                    }
                    if let Data::Recovery { last_success, .. } = &mut old.data {
                        *last_success = combined;
                    }
                }
                continue;
            }
            indices.insert(obs.resource.clone(), output.len());
        }
        output.push(obs);
    }
    *observations = output;
}

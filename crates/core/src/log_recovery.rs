//! Clear historical diagnostic findings only with distinct, complete replacement windows.
use crate::{config::resolve::Job, model::*};
pub fn reconcile(job: &Job, snapshot: &Snapshot, result: &mut CheckResult) {
    if job.check != Check::Logs {
        return;
    }
    let mut clears = Vec::new();
    for finding in snapshot.findings.values().filter(|finding| {
        finding.rule == "runtime-failure-sample"
            && finding
                .resource
                .starts_with(&format!("{}/", job.target.name))
    }) {
        if result
            .observations
            .iter()
            .any(|obs| obs.resource == finding.resource)
        {
            continue;
        }
        let window=result.observations.iter().find(|obs| finding.evidence.contains(&obs.operation) && matches!(obs.data,Data::LogWindow{complete:true,gap_seconds:0,end,..}if end>finding.observed_at) && result.operations.iter().any(|op|op.id==obs.operation && op.coverage==Coverage::Complete));
        let Some(window) = window else {
            continue;
        };
        if result.observations.len() + clears.len() >= job.settings.max_assets {
            break;
        }
        let signature = snapshot
            .results
            .get(&job.key)
            .and_then(|result| {
                result
                    .observations
                    .iter()
                    .find(|obs| obs.resource == finding.resource)
            })
            .and_then(|obs| match obs.data {
                Data::Log { signature, .. } => Some(signature),
                _ => None,
            })
            .unwrap_or(LogClass::OtherError);
        clears.push(Observation {
            resource: finding.resource.clone(),
            operation: window.operation.clone(),
            observed_at: window.observed_at,
            expected: finding.expected,
            data: Data::Log {
                signature,
                count: 0,
                first_seen: finding.observed_at,
                last_seen: finding.observed_at,
                sampled: false,
            },
        });
    }
    result.observations.extend(clears);
}

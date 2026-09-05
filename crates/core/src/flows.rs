//! Aggregate pipeline progress without database records or synthetic transactions.
use crate::{
    config::{resolve::Job, types::SignalMode},
    model::*,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    value: f64,
    last_seen: DateTime<Utc>,
    last_progress: DateTime<Utc>,
    established: bool,
}
pub fn evaluate(snapshot: &mut Snapshot, job: &Job, now: DateTime<Utc>) -> CheckResult {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        Check::Flows,
        job.revision.clone(),
        Coverage::Missing,
    );
    result.operations.clear();
    for flow in &job.target.flows {
        let operation = format!("flow/{}", flow.name);
        let demand = signal(snapshot, job, &flow.demand, now);
        let mut complete = true;
        for stage in &flow.stages {
            let key = format!("{}/flows/{}/{}", job.target.name, flow.name, stage.name);
            let progress = signal(snapshot, job, &stage.progress, now);
            let (state, at) = if job.target.expected != Expected::Active {
                (Health::ExpectedInactive, now)
            } else if let Some((demand, at)) = demand.filter(|(value, _)| *value <= 0.0) {
                let _ = demand;
                snapshot.progress.remove(&key);
                (Health::ExpectedInactive, at)
            } else if let (Some((_, demand_at)), Some((value, at))) = (demand, progress) {
                let at = at.min(demand_at);
                let previous = snapshot.progress.get(&key).cloned();
                let mut state = Health::Unknown;
                if let Some(mut previous) = previous {
                    if at > previous.last_seen {
                        if (at - previous.last_seen).num_seconds() > job.settings.freshness() as i64
                            || value < previous.value && matches!(stage.mode, SignalMode::Counter)
                        {
                            previous.last_progress = at;
                            previous.established = false;
                        } else {
                            previous.established = true;
                            if match stage.mode {
                                SignalMode::Counter => value > previous.value,
                                SignalMode::Rate => value > 0.0,
                            } {
                                previous.last_progress = at;
                            }
                        }
                        previous.value = value;
                        previous.last_seen = at;
                        snapshot.progress.insert(key.clone(), previous.clone());
                    }
                    if previous.established {
                        state = if (at - previous.last_progress).num_seconds()
                            >= flow.idle_after.0 as i64
                        {
                            Health::Unhealthy
                        } else {
                            Health::Healthy
                        };
                    }
                } else if snapshot.progress.len() < job.settings.max_findings {
                    snapshot.progress.insert(
                        key.clone(),
                        Progress {
                            value,
                            last_seen: at,
                            last_progress: at,
                            established: false,
                        },
                    );
                    if matches!(stage.mode, SignalMode::Rate) && value > 0.0 {
                        state = Health::Healthy;
                    }
                } else {
                    complete = false;
                }
                if let Some(workload) = &stage.workload
                    && !workload_known(snapshot, job, workload, now)
                {
                    complete = false;
                    state = Health::Unknown;
                }
                (state, at)
            } else {
                complete = false;
                (Health::Unknown, now)
            };
            result.observations.push(Observation {
                resource: key,
                operation: operation.clone(),
                observed_at: at,
                expected: job.target.expected,
                data: Data::Progress { state },
            });
        }
        result.operations.push(Operation {
            id: operation,
            coverage: if complete {
                Coverage::Complete
            } else {
                Coverage::Missing
            },
            observed_at: now,
            records: flow.stages.len(),
            pages: 0,
            attempts: 0,
            required: true,
        });
    }
    result.finished_at = now;
    result
}
fn signal(
    snapshot: &Snapshot,
    job: &Job,
    selector: &str,
    now: DateTime<Utc>,
) -> Option<(f64, DateTime<Utc>)> {
    let mut found = None;
    let mut resource = None;
    for result in snapshot
        .results
        .values()
        .filter(|r| r.target == job.target.name)
    {
        for observation in &result.observations {
            let matches = observation.resource == selector
                || observation.resource.ends_with(&format!("/{selector}"))
                || matches!(&observation.data,Data::Metric{name,..}if name==selector);
            if !matches
                || !result
                    .operations
                    .iter()
                    .any(|o| o.id == observation.operation && o.coverage == Coverage::Complete)
            {
                continue;
            }
            if (now - observation.observed_at).num_seconds() > job.settings.freshness() as i64
                || observation.observed_at > now
            {
                continue;
            }
            let value = match &observation.data {
                Data::Metric { value, .. } => *value,
                Data::Queue { backlog, .. } => *backlog,
                _ => continue,
            };
            if !value.is_finite() {
                continue;
            }
            if resource.is_some_and(|r| r != &observation.resource) {
                return None;
            }
            resource = Some(&observation.resource);
            if found.is_none_or(|(_, at)| observation.observed_at > at) {
                found = Some((value, observation.observed_at));
            }
        }
    }
    found
}
fn workload_known(snapshot: &Snapshot, job: &Job, selector: &str, now: DateTime<Utc>) -> bool {
    snapshot
        .results
        .values()
        .filter(|r| r.target == job.target.name)
        .any(|r| {
            r.observations.iter().any(|o| {
                o.resource.ends_with(&format!("/{selector}"))
                    && matches!(o.data, Data::Workload { .. })
                    && r.operations
                        .iter()
                        .any(|op| op.id == o.operation && op.coverage == Coverage::Complete)
                    && o.observed_at <= now
                    && (now - o.observed_at).num_seconds() <= job.settings.freshness() as i64
            })
        })
}

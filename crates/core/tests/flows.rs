//! Progress checks distinguish ready pods from completed work.
use chrono::{DateTime, Duration, Utc};
use monitor_core::{
    config::{
        resolve::{Job, Selection},
        types::Config,
    },
    flows,
    model::*,
    state::State,
};
fn job() -> Result<Job, Box<dyn std::error::Error>> {
    let config = Config::parse(
        "version=1\n[[targets]]\nname='dev'\nprovider='gcp'\nscope='dev-project'\n[[targets.flows]]\nname='scanning'\ndemand='incoming'\nidle_after='90s'\n[[targets.flows.stages]]\nname='query-producer'\nprogress='completed'\nmode='counter'\nworkload='workers/query-producer'",
    )?;
    config
        .resolve(&Selection::default())?
        .jobs
        .into_iter()
        .find(|j| j.check == Check::Flows)
        .ok_or_else(|| "missing flow job".into())
}
fn signals(
    snapshot: &mut Snapshot,
    job: &Job,
    at: DateTime<Utc>,
    demand: f64,
    progress: Option<f64>,
) {
    let mut observations = vec![
        Observation {
            resource: "dev/incoming".into(),
            operation: "telemetry".into(),
            observed_at: at,
            expected: Expected::Active,
            data: Data::Metric {
                name: "incoming".into(),
                value: demand,
                capacity: None,
                warning: None,
                error: None,
                window_seconds: 60,
            },
        },
        Observation {
            resource: "dev/workers/query-producer".into(),
            operation: "telemetry".into(),
            observed_at: at,
            expected: Expected::Active,
            data: Data::Workload {
                desired: 1,
                ready: 1,
                created_at: None,
                draining: false,
                node: false,
            },
        },
    ];
    if let Some(value) = progress {
        observations.push(Observation {
            resource: "dev/completed".into(),
            operation: "telemetry".into(),
            observed_at: at,
            expected: Expected::Active,
            data: Data::Metric {
                name: "completed".into(),
                value,
                capacity: None,
                warning: None,
                error: None,
                window_seconds: 60,
            },
        });
    }
    snapshot.results.insert(
        "dev/Metrics".into(),
        CheckResult {
            target: "dev".into(),
            check: Check::Metrics,
            revision: job.revision.clone(),
            started_at: at,
            finished_at: at,
            operations: vec![Operation {
                id: "telemetry".into(),
                coverage: Coverage::Complete,
                observed_at: at,
                records: observations.len(),
                pages: 1,
                attempts: 1,
                required: true,
            }],
            observations,
        },
    );
}
fn state(result: &CheckResult) -> Option<Health> {
    result.observations.iter().find_map(|o| match o.data {
        Data::Progress { state } => Some(state),
        _ => None,
    })
}
#[test]
fn stalled_stage_is_detected_even_with_ready_pods() -> Result<(), Box<dyn std::error::Error>> {
    let job = job()?;
    let mut snapshot = State::new(job.revision.clone(), vec![]).snapshot;
    let now = Utc::now();
    signals(&mut snapshot, &job, now, 10.0, Some(12.0));
    assert_eq!(
        state(&flows::evaluate(&mut snapshot, &job, now)),
        Some(Health::Unknown)
    );
    let later = now + Duration::seconds(120);
    signals(&mut snapshot, &job, later, 10.0, Some(12.0));
    assert_eq!(
        state(&flows::evaluate(&mut snapshot, &job, later)),
        Some(Health::Unhealthy)
    );
    Ok(())
}
#[test]
fn new_completions_show_progress() -> Result<(), Box<dyn std::error::Error>> {
    let job = job()?;
    let mut snapshot = State::new(job.revision.clone(), vec![]).snapshot;
    let now = Utc::now();
    signals(&mut snapshot, &job, now, 10.0, Some(12.0));
    flows::evaluate(&mut snapshot, &job, now);
    let later = now + Duration::seconds(120);
    signals(&mut snapshot, &job, later, 10.0, Some(13.0));
    assert_eq!(
        state(&flows::evaluate(&mut snapshot, &job, later)),
        Some(Health::Healthy)
    );
    Ok(())
}
#[test]
fn zero_input_is_expected_inactive_without_synthetic_work() -> Result<(), Box<dyn std::error::Error>>
{
    let job = job()?;
    let mut snapshot = State::new(job.revision.clone(), vec![]).snapshot;
    let now = Utc::now();
    signals(&mut snapshot, &job, now, 0.0, None);
    assert_eq!(
        state(&flows::evaluate(&mut snapshot, &job, now)),
        Some(Health::ExpectedInactive)
    );
    Ok(())
}
#[test]
fn missing_completion_signal_is_unknown_and_incomplete() -> Result<(), Box<dyn std::error::Error>> {
    let job = job()?;
    let mut snapshot = State::new(job.revision.clone(), vec![]).snapshot;
    let now = Utc::now();
    signals(&mut snapshot, &job, now, 1.0, None);
    let result = flows::evaluate(&mut snapshot, &job, now);
    assert_eq!(state(&result), Some(Health::Unknown));
    assert!(!result.complete());
    Ok(())
}
#[test]
fn collection_gap_cannot_prove_continuous_stalling() -> Result<(), Box<dyn std::error::Error>> {
    let job = job()?;
    let mut snapshot = State::new(job.revision.clone(), vec![]).snapshot;
    let now = Utc::now();
    signals(&mut snapshot, &job, now, 10.0, Some(12.0));
    flows::evaluate(&mut snapshot, &job, now);
    let later = now + Duration::hours(1);
    signals(&mut snapshot, &job, later, 10.0, Some(12.0));
    assert_eq!(
        state(&flows::evaluate(&mut snapshot, &job, later)),
        Some(Health::Unknown)
    );
    Ok(())
}
#[test]
fn counter_reset_is_not_a_stalled_stage() -> Result<(), Box<dyn std::error::Error>> {
    let job = job()?;
    let mut snapshot = State::new(job.revision.clone(), vec![]).snapshot;
    let now = Utc::now();
    signals(&mut snapshot, &job, now, 10.0, Some(12.0));
    flows::evaluate(&mut snapshot, &job, now);
    let later = now + Duration::seconds(120);
    signals(&mut snapshot, &job, later, 10.0, Some(1.0));
    assert_eq!(
        state(&flows::evaluate(&mut snapshot, &job, later)),
        Some(Health::Unknown)
    );
    Ok(())
}

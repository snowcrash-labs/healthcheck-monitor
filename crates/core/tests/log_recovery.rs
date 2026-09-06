//! Empty complete windows can retire diagnostics; capped or missing windows cannot.
use chrono::{DateTime, Duration, Utc};
use monitor_core::{
    config::{
        resolve::{Job, Selection},
        types::Config,
    },
    model::*,
    state::State,
};
fn job() -> Result<Job, Box<dyn std::error::Error>> {
    Config::parse("version=1\n[[targets]]\nname='dev'\nprovider='gcp'\nscope='project'")?
        .resolve(&Selection::default())?
        .jobs
        .into_iter()
        .find(|job| job.check == Check::Logs)
        .ok_or_else(|| "missing job".into())
}
fn result(job: &Job, at: DateTime<Utc>, coverage: Coverage, error: bool) -> CheckResult {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        coverage,
    );
    result.operations[0].id = "errors".into();
    result.operations[0].observed_at = at;
    result.observations.push(Observation {
        context: None,
        resource: "dev/errors/window".into(),
        operation: "errors".into(),
        observed_at: at,
        expected: Expected::Active,
        data: Data::LogWindow {
            gap_seconds: 0,
            start: at - Duration::minutes(5),
            end: at,
            scanned: usize::from(error),
            duplicates: 0,
            limit: 500,
            complete: coverage == Coverage::Complete,
        },
    });
    if error {
        result.observations.push(Observation {
            context: None,
            resource: "dev/errors/worker/Import".into(),
            operation: "errors".into(),
            observed_at: at,
            expected: Expected::Active,
            data: Data::Log {
                signature: LogClass::Import,
                count: 1,
                first_seen: at,
                last_seen: at,
                sampled: false,
            },
        });
    }
    result
}
#[test]
fn only_two_distinct_complete_windows_clear_a_diagnostic() -> Result<(), Box<dyn std::error::Error>>
{
    let job = job()?;
    let now = Utc::now();
    let mut state = State::new(job.revision.clone(), vec![job.key.clone()]);
    state.apply(&job, result(&job, now, Coverage::Complete, true), now);
    for second in 1..=3 {
        let at = now + Duration::seconds(second);
        state.apply(&job, result(&job, at, Coverage::Truncated, false), at);
    }
    assert_eq!(state.snapshot.findings.len(), 1);
    let at = now + Duration::seconds(4);
    let clear = result(&job, at, Coverage::Complete, false);
    state.apply(&job, clear.clone(), at);
    state.apply(&job, clear, at);
    assert_eq!(state.snapshot.findings.len(), 1);
    let at = now + Duration::seconds(5);
    let transitions = state.apply(&job, result(&job, at, Coverage::Complete, false), at);
    assert!(state.snapshot.findings.is_empty());
    assert!(
        transitions
            .iter()
            .any(|transition| transition.kind == TransitionKind::Recovered)
    );
    Ok(())
}

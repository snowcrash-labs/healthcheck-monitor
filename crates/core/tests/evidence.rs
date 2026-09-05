//! Recovery, incomplete collection, scope, restart, and persistence contracts.
use chrono::{Duration, Utc};
use monitor_core::{
    config::{
        resolve::{Job, Selection},
        settings::Settings,
        types::Config,
    },
    model::*,
    state::State,
    storage::Store,
};
fn job() -> Result<Job, Box<dyn std::error::Error>> {
    let config =
        Config::parse("version=1\n[[targets]]\nname='dev'\nprovider='edge'\nscope='public'")?;
    config
        .resolve(&Selection::default())?
        .jobs
        .into_iter()
        .next()
        .ok_or_else(|| "no job".into())
}
fn result(job: &Job, healthy: bool, at: chrono::DateTime<Utc>, coverage: Coverage) -> CheckResult {
    CheckResult {
        target: job.target.name.clone(),
        check: job.check,
        revision: job.revision.clone(),
        started_at: at,
        finished_at: at,
        operations: vec![Operation {
            id: "status".into(),
            coverage,
            observed_at: at,
            records: 1,
            pages: 1,
            attempts: 1,
            required: true,
        }],
        observations: vec![Observation {
            resource: "dev/status/api".into(),
            operation: "status".into(),
            observed_at: at,
            expected: Expected::Active,
            data: Data::Condition {
                rule: "unavailable".into(),
                healthy: Some(healthy),
            },
        }],
    }
}
#[test]
fn fresh_two_sample_recovery_required() -> Result<(), Box<dyn std::error::Error>> {
    let job = job()?;
    let now = Utc::now();
    let mut state = State::new(job.revision.clone(), vec![job.key.clone()]);
    state.apply(&job, result(&job, false, now, Coverage::Complete), now);
    assert_eq!(state.snapshot.findings.len(), 1);
    let first = now + Duration::seconds(1);
    state.apply(&job, result(&job, true, first, Coverage::Complete), first);
    assert_eq!(state.snapshot.findings.len(), 1);
    state.apply(&job, result(&job, true, first, Coverage::Complete), first);
    assert_eq!(state.snapshot.findings.len(), 1);
    let second = first + Duration::seconds(1);
    state.apply(&job, result(&job, true, second, Coverage::Complete), second);
    assert!(state.snapshot.findings.is_empty());
    Ok(())
}
#[test]
fn denied_or_truncated_success_cannot_clear_findings() -> Result<(), Box<dyn std::error::Error>> {
    let job = job()?;
    let now = Utc::now();
    let mut state = State::new(job.revision.clone(), vec![job.key.clone()]);
    state.apply(&job, result(&job, false, now, Coverage::Complete), now);
    for coverage in [
        Coverage::Denied,
        Coverage::Truncated,
        Coverage::Stale,
        Coverage::Missing,
    ] {
        for i in 1..=3 {
            let at = now + Duration::seconds(i);
            state.apply(&job, result(&job, true, at, coverage), at);
        }
    }
    assert_eq!(state.snapshot.findings.len(), 1);
    Ok(())
}
#[test]
fn two_complete_inventories_confirm_removal() -> Result<(), Box<dyn std::error::Error>> {
    let mut job = job()?;
    job.check = Check::Inventory;
    job.key = "dev/Inventory".into();
    let now = Utc::now();
    let mut state = State::new(job.revision.clone(), vec![job.key.clone()]);
    state.apply(&job, result(&job, false, now, Coverage::Complete), now);
    for i in 1..=2 {
        let at = now + Duration::seconds(i);
        let mut r = result(&job, true, at, Coverage::Complete);
        r.observations.clear();
        state.apply(&job, r, at);
        if i == 1 {
            assert_eq!(state.snapshot.findings.len(), 1);
        }
    }
    assert!(state.snapshot.findings.is_empty());
    Ok(())
}
#[test]
fn coverage_precedes_health_exit_status() -> Result<(), Box<dyn std::error::Error>> {
    let job = job()?;
    let now = Utc::now();
    let mut state = State::new(job.revision.clone(), vec![job.key.clone()]);
    state.apply(&job, result(&job, false, now, Coverage::Complete), now);
    assert_eq!(monitor_core::report::exit_code(&state.snapshot, false), 1);
    state.apply(&job, result(&job, false, now, Coverage::Denied), now);
    assert_eq!(monitor_core::report::exit_code(&state.snapshot, false), 3);
    Ok(())
}
#[test]
fn atomic_snapshots_lock_and_restore_original_age() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = Store::open(directory.path())?;
    assert!(Store::open(directory.path()).is_err());
    let job = job()?;
    let now = Utc::now() - Duration::hours(2);
    let mut state = State::new(job.revision.clone(), vec![job.key.clone()]);
    state.apply(&job, result(&job, false, now, Coverage::Complete), now);
    store.publish(&state.snapshot, &[], &Settings::default())?;
    let restored = store.latest(1024 * 1024)?.ok_or("missing snapshot")?;
    assert_eq!(restored.captured_at, now);
    std::fs::write(directory.path().join(".monitor-latest.json.tmp"), "{broken")?;
    assert!(store.latest(1024 * 1024)?.is_some());
    Ok(())
}
#[test]
fn retention_only_removes_service_history() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = Store::open(directory.path())?;
    std::fs::write(directory.path().join("operator-notes.txt"), "keep")?;
    let settings = Settings {
        history_count: 2,
        ..Default::default()
    };
    let mut state = State::new("revision".into(), vec![]);
    for i in 0..5 {
        state.snapshot.captured_at = Utc::now() + Duration::seconds(i);
        store.publish(&state.snapshot, &[], &settings)?;
    }
    let count = std::fs::read_dir(directory.path())?
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("monitor-snapshot-")
        })
        .count();
    assert_eq!(count, 2);
    assert!(directory.path().join("operator-notes.txt").exists());
    Ok(())
}
#[test]
fn structural_diff_ignores_timestamp_changes() -> Result<(), Box<dyn std::error::Error>> {
    let job = job()?;
    let now = Utc::now();
    let mut state = State::new(job.revision.clone(), vec![job.key.clone()]);
    state.apply(&job, result(&job, false, now, Coverage::Complete), now);
    let old = state.snapshot.clone();
    let at = now + Duration::seconds(1);
    state.apply(&job, result(&job, false, at, Coverage::Complete), at);
    assert!(monitor_core::report::diff(&old, &state.snapshot).is_empty());
    Ok(())
}

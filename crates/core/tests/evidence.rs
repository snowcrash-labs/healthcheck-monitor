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

#[test]
fn cached_inventory_and_reload_preserve_pending_removal() -> Result<(), Box<dyn std::error::Error>>
{
    let mut job = job()?;
    job.check = Check::Inventory;
    let at = Utc::now();
    let mut state = State::new(job.revision.clone(), vec![job.key.clone()]);
    state.apply(&job, result(&job, false, at, Coverage::Complete), at);
    let later = at + Duration::seconds(1);
    let mut empty = result(&job, true, later, Coverage::Complete);
    empty.observations.clear();
    state.apply(&job, empty.clone(), later);
    state.retain_scope(&[job.clone()]);
    state.apply(&job, empty.clone(), later + Duration::seconds(1));
    assert_eq!(state.snapshot.findings.len(), 1);
    let restored = serde_json::from_slice(&serde_json::to_vec(&state.snapshot)?)?;
    state = State { snapshot: restored };
    empty.operations[0].observed_at = later + Duration::seconds(2);
    let transitions = state.apply(&job, empty, later + Duration::seconds(2));
    assert!(state.snapshot.findings.is_empty());
    assert!(
        transitions
            .iter()
            .any(|t| t.kind == TransitionKind::Removed)
    );
    let transitions = state.apply(&job, result(&job, false, later, Coverage::Complete), later);
    assert!(
        transitions
            .iter()
            .any(|t| t.kind == TransitionKind::Reappeared)
    );
    Ok(())
}

#[test]
fn single_run_does_not_infer_persistence_from_saved_queue_sample()
-> Result<(), Box<dyn std::error::Error>> {
    let mut job = job()?;
    job.check = Check::Queues;
    let at = Utc::now();
    let mut r = result(&job, true, at, Coverage::Complete);
    r.observations[0].data = Data::Queue {
        backlog: 20.0,
        activation: 0.0,
        ready: 0,
        desired: 1,
        crash_loop: false,
        scaler_ready: true,
        age_seconds: None,
        dead_letters: None,
    };
    let mut state = State::new(job.revision.clone(), vec![job.key.clone()]);
    state.apply(&job, r.clone(), at);
    state.begin_run();
    let later = at + Duration::seconds(30);
    r.observations[0].observed_at = later;
    state.apply(&job, r, later);
    assert!(state.snapshot.findings.is_empty());
    assert_eq!(
        state.snapshot.health.values().next(),
        Some(&Health::Unknown)
    );
    assert_eq!(state.snapshot.samples.get(&job.key), Some(&1));
    Ok(())
}

#[test]
fn unrelated_inventory_cannot_confirm_another_checks_removal()
-> Result<(), Box<dyn std::error::Error>> {
    let mut inventory = job()?;
    inventory.check = Check::Inventory;
    let mut kube = inventory.clone();
    kube.check = Check::Kubernetes;
    kube.key = "dev/Kubernetes".into();
    let at = Utc::now();
    let mut state = State::new(
        inventory.revision.clone(),
        vec![inventory.key.clone(), kube.key.clone()],
    );
    state.apply(
        &inventory,
        result(&inventory, false, at, Coverage::Complete),
        at,
    );
    for second in 1..=3 {
        let at = at + Duration::seconds(second);
        let mut empty = result(&kube, true, at, Coverage::Complete);
        empty.observations.clear();
        state.apply(&kube, empty, at);
    }
    assert_eq!(state.snapshot.findings.len(), 1);
    Ok(())
}

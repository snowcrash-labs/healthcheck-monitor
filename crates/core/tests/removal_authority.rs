//! Recovery and removal require complete evidence from the resource's own inventory.
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
    Config::parse("version=1\n[[targets]]\nname='dev'\nprovider='gcp'\nscope='project'\nregions=['us-central1']")?.resolve(&Selection{checks:vec![Check::Inventory],..Default::default()})?.jobs.into_iter().find(|job|job.check==Check::Inventory).ok_or_else(||"missing job".into())
}
fn observation(name: &str, healthy: bool, at: DateTime<Utc>) -> Observation {
    Observation {
        resource: format!("dev/status/{name}"),
        operation: "status".into(),
        observed_at: at,
        expected: Expected::Active,
        data: Data::Condition {
            rule: "unavailable".into(),
            healthy: Some(healthy),
        },
    }
}
fn result(job: &Job, at: DateTime<Utc>, observations: Vec<Observation>) -> CheckResult {
    CheckResult {
        target: job.target.name.clone(),
        check: job.check,
        revision: job.revision.clone(),
        started_at: at,
        finished_at: at,
        operations: vec![Operation {
            id: "status".into(),
            coverage: Coverage::Complete,
            observed_at: at,
            records: observations.len(),
            pages: 1,
            attempts: 1,
            required: true,
        }],
        observations,
    }
}
#[test]
fn optional_inventory_failure_cannot_remove_a_finding_or_erase_its_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let job = job()?;
    let now = Utc::now();
    let mut state = State::new(job.revision.clone(), vec![job.key.clone()]);
    state.apply(
        &job,
        result(&job, now, vec![observation("service", false, now)]),
        now,
    );
    for second in 1..=3 {
        let at = now + Duration::seconds(second);
        let mut failed = result(&job, at, vec![]);
        failed.operations[0].required = false;
        failed.operations[0].coverage = Coverage::Unavailable;
        assert!(failed.complete());
        state.apply(&job, failed, at);
    }
    assert_eq!(state.snapshot.findings.len(), 1);
    assert!(state.snapshot.confirmations.is_empty());
    assert_eq!(
        state
            .snapshot
            .results
            .get(&job.key)
            .ok_or("missing result")?
            .observations[0]
            .observed_at,
        now
    );
    Ok(())
}
#[test]
fn another_failed_api_does_not_block_two_complete_resource_inventories()
-> Result<(), Box<dyn std::error::Error>> {
    let job = job()?;
    let now = Utc::now();
    let mut state = State::new(job.revision.clone(), vec![job.key.clone()]);
    state.apply(
        &job,
        result(&job, now, vec![observation("service", false, now)]),
        now,
    );
    for second in 1..=2 {
        let at = now + Duration::seconds(second);
        let mut empty = result(&job, at, vec![]);
        let mut other = empty.operations[0].clone();
        other.id = "other-api".into();
        other.coverage = Coverage::Denied;
        empty.operations.push(other);
        assert!(!empty.complete());
        state.apply(&job, empty, at);
    }
    assert!(state.snapshot.findings.is_empty());
    assert!(
        state
            .snapshot
            .retired
            .values()
            .any(|transition| transition.kind == TransitionKind::Removed)
    );
    Ok(())
}
#[test]
fn a_finding_limit_reached_late_in_a_scan_invalidates_earlier_clear_decisions()
-> Result<(), Box<dyn std::error::Error>> {
    let mut job = job()?;
    job.settings.max_findings = 1;
    job.settings.recover_confirmations = 1;
    let now = Utc::now();
    let mut state = State::new(job.revision.clone(), vec![job.key.clone()]);
    state.apply(
        &job,
        result(&job, now, vec![observation("first", false, now)]),
        now,
    );
    let at = now + Duration::seconds(1);
    state.apply(
        &job,
        result(
            &job,
            at,
            vec![
                observation("first", true, at),
                observation("second", false, at),
            ],
        ),
        at,
    );
    assert!(
        state
            .snapshot
            .findings
            .values()
            .any(|finding| finding.resource.ends_with("/first"))
    );
    assert_eq!(
        state.snapshot.health.get("dev/status/first"),
        Some(&Health::Unknown)
    );
    Ok(())
}
#[test]
fn default_asset_limit_can_be_compared_across_successive_complete_inventories()
-> Result<(), Box<dyn std::error::Error>> {
    let job = job()?;
    let now = Utc::now();
    let mut state = State::new(job.revision.clone(), vec![job.key.clone()]);
    for second in 0..2 {
        let at = now + Duration::seconds(second);
        let observations = (0..job.settings.max_assets)
            .map(|index| observation(&format!("service-{index}"), true, at))
            .collect();
        state.apply(&job, result(&job, at, observations), at);
    }
    assert_eq!(state.snapshot.health.len(), 50_000);
    assert!(
        state
            .snapshot
            .results
            .get(&job.key)
            .is_some_and(CheckResult::complete)
    );
    assert!(state.snapshot.findings.is_empty());
    Ok(())
}

//! Cancellation and failed refreshes retain original evidence but cannot establish current health.
use chrono::{Duration, Utc};
use monitor_core::{
    config::{resolve::Selection, types::Config},
    model::*,
    state::State,
};
#[test]
fn cancelled_endpoint_refresh_retains_last_observation_without_refreshing_its_age()
-> Result<(), Box<dyn std::error::Error>> {
    let job = Config::parse("version=1\n[[targets]]\nname='dev'\nprovider='edge'\nscope='public'")?
        .resolve(&Selection::default())?
        .jobs
        .into_iter()
        .find(|job| job.check == Check::Edge)
        .ok_or("edge")?;
    let at = Utc::now();
    let mut result = CheckResult::failure(
        "dev".into(),
        Check::Edge,
        job.revision.clone(),
        Coverage::Complete,
    );
    result.operations[0].id = "https".into();
    result.operations[0].observed_at = at;
    result.observations.push(Observation {
        context: None,
        resource: "dev/https/api".into(),
        operation: "https".into(),
        observed_at: at,
        expected: Expected::Active,
        data: Data::Endpoint {
            dns: true,
            tls: true,
            status: Some(200),
            accepted: vec![200],
            latency_ms: 20,
            expires_at: None,
        },
    });
    let mut state = State::new(job.revision.clone(), vec![job.key.clone()]);
    state.apply(&job, result, at);
    let cancelled = CheckResult::failure(
        "dev".into(),
        Check::Edge,
        job.revision.clone(),
        Coverage::Cancelled,
    );
    let later = at + Duration::seconds(30);
    state.apply(&job, cancelled, later);
    let result = state.snapshot.results.get(&job.key).ok_or("result")?;
    assert_eq!(result.observations.len(), 1);
    assert_eq!(result.observations[0].observed_at, at);
    assert_eq!(
        state.snapshot.health.get("dev/https/api"),
        Some(&Health::Unknown)
    );
    assert!(!result.complete());
    Ok(())
}
#[test]
fn changing_provider_scope_drops_old_baselines_without_claiming_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    let mut job =
        Config::parse("version=1\n[[targets]]\nname='dev'\nprovider='edge'\nscope='old'")?
            .resolve(&Selection::default())?
            .jobs
            .into_iter()
            .find(|job| job.check == Check::Edge)
            .ok_or("job")?;
    let at = Utc::now();
    let mut result = CheckResult::failure(
        "dev".into(),
        Check::Edge,
        job.revision.clone(),
        Coverage::Complete,
    );
    result.observations.push(Observation {
        context: None,
        resource: "dev/service".into(),
        operation: "Edge".into(),
        observed_at: at,
        expected: Expected::Active,
        data: Data::Condition {
            rule: "unavailable".into(),
            healthy: Some(false),
        },
    });
    let mut state = State::new(job.revision.clone(), vec![job.key.clone()]);
    state.apply(&job, result, at);
    let old = state.snapshot.clone();
    job.target.scope = "new".into();
    job.revision = "changed".into();
    state.retain_scope(&[job]);
    assert!(state.snapshot.results.is_empty());
    assert!(state.snapshot.findings.is_empty());
    assert!(state.snapshot.health.is_empty());
    assert!(monitor_core::report::diff(&old, &state.snapshot).is_empty());
    Ok(())
}

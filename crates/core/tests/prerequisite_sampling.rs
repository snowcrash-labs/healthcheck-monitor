//! Repeated checks obtain fresh prerequisites while explicit sample overrides remain authoritative.
use monitor_core::{
    config::{resolve::Selection, types::Config},
    model::*,
};
#[test]
fn queue_sampling_repeats_default_kubernetes_prerequisites()
-> Result<(), Box<dyn std::error::Error>> {
    let config = Config::parse(
        "version=1\n[[targets]]\nname='test'\nprovider='gcp'\nscope='project'\ncontext='context'",
    )?;
    let effective = config.resolve(&Selection {
        checks: vec![Check::Queues],
        ..Default::default()
    })?;
    assert!(
        !effective
            .jobs
            .iter()
            .any(|job| job.check == Check::Inventory)
    );
    assert!(effective.jobs.iter().all(|job| job.kube_only));
    assert!(
        effective
            .jobs
            .iter()
            .any(|job| job.check == Check::Kubernetes && !job.assess_health)
    );
    assert_eq!(
        effective
            .jobs
            .iter()
            .find(|job| job.check == Check::Kubernetes)
            .map(|job| job.settings.samples),
        Some(5)
    );
    let config = Config::parse(
        "version=1\n[[targets]]\nname='test'\nprovider='gcp'\nscope='project'\ncontext='context'\n[targets.checks.kubernetes]\nsamples=1",
    )?;
    let effective = config.resolve(&Selection {
        checks: vec![Check::Queues],
        ..Default::default()
    })?;
    assert_eq!(
        effective
            .jobs
            .iter()
            .find(|job| job.check == Check::Kubernetes)
            .map(|job| job.settings.samples),
        Some(1)
    );
    Ok(())
}
#[test]
fn required_unmapped_flows_are_visible_without_polling_unused_metrics_faster()
-> Result<(), Box<dyn std::error::Error>> {
    let config = Config::parse(
        "version=1\n[[targets]]\nname='test'\nprovider='gcp'\nscope='project'\nflows_required=true",
    )?;
    let effective = config.resolve(&Selection::default())?;
    let job = effective
        .jobs
        .iter()
        .find(|job| job.check == Check::Flows)
        .ok_or("flow")?;
    let mut state = monitor_core::state::State::new(job.revision.clone(), vec![]);
    let result = monitor_core::flows::evaluate(&mut state.snapshot, job, chrono::Utc::now());
    assert_eq!(result.operations[0].coverage, Coverage::Missing);
    assert_eq!(
        effective
            .jobs
            .iter()
            .find(|job| job.check == Check::Metrics)
            .map(|job| job.settings.interval.0),
        Some(300)
    );
    Ok(())
}
#[test]
fn collection_prerequisites_do_not_add_unrequested_health_findings()
-> Result<(), Box<dyn std::error::Error>> {
    let config = Config::parse(
        "version=1\n[[targets]]\nname='test'\nprovider='gcp'\nscope='project'\ncontext='context'",
    )?;
    let effective = config.resolve(&Selection {
        checks: vec![Check::Queues],
        ..Default::default()
    })?;
    let job = effective
        .jobs
        .iter()
        .find(|job| job.check == Check::Kubernetes)
        .ok_or("kube")?;
    let at = chrono::Utc::now();
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Complete,
    );
    result.observations.push(Observation {
        resource: "test/metadata/unrelated".into(),
        operation: "Kubernetes".into(),
        observed_at: at,
        expected: Expected::Active,
        data: Data::Condition {
            rule: "unrelated-failure".into(),
            healthy: Some(false),
        },
    });
    let mut state = monitor_core::state::State::new(
        job.revision.clone(),
        effective.jobs.iter().map(|job| job.key.clone()).collect(),
    );
    state.apply(job, result, at);
    assert!(state.snapshot.findings.is_empty());
    assert!(state.snapshot.collection_only.contains(&job.key));
    assert!(monitor_core::report::markdown(&state.snapshot).contains("Kubernetes (prerequisite)"));
    Ok(())
}

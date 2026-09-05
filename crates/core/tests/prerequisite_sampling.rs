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

//! Budget reductions validate against retained evidence before replacing active state.
use monitor_core::{
    config::{resolve::Selection, types::Config},
    model::*,
    state::State,
};
#[test]
fn a_smaller_budget_is_rejected_atomically_until_the_selected_scope_fits()
-> Result<(), Box<dyn std::error::Error>> {
    let config = Config::parse(
        "version=1\n[[targets]]\nname='test'\nprovider='gcp'\nscope='project'\nregions=['us-central1']",
    )?;
    let job = config
        .resolve(&Selection {
            checks: vec![Check::Inventory],
            ..Default::default()
        })?
        .jobs
        .into_iter()
        .find(|job| job.check == Check::Inventory)
        .ok_or("missing inventory")?;
    let mut state = State::new(job.revision.clone(), vec![job.key.clone()])
        .reconfigured(std::slice::from_ref(&job))?;
    let now = chrono::Utc::now();
    let mut result = CheckResult::failure(
        "test".into(),
        Check::Inventory,
        job.revision.clone(),
        Coverage::Complete,
    );
    result.operations[0].id = "inventory".into();
    result.observations = ["one", "two"]
        .into_iter()
        .map(|name| Observation {
            resource: format!("test/inventory/{name}"),
            operation: "inventory".into(),
            observed_at: now,
            expected: Expected::Active,
            data: Data::Condition {
                rule: "unavailable".into(),
                healthy: Some(false),
            },
        })
        .collect();
    state.apply(&job, result, now);
    let mut smaller = job.clone();
    smaller.settings.max_assets = 1;
    smaller.settings.max_findings = 1;
    smaller.revision = "replacement".into();
    assert!(state.reconfigured(std::slice::from_ref(&smaller)).is_err());
    assert_eq!(state.snapshot.findings.len(), 2);
    assert_eq!(state.snapshot.revision, job.revision);
    smaller.target.resources = vec!["one".into()];
    let replacement = state.reconfigured(&[smaller])?;
    assert!(replacement.snapshot.results.is_empty());
    assert!(replacement.snapshot.retired.is_empty());
    assert_eq!(replacement.snapshot.revision, "replacement");
    Ok(())
}

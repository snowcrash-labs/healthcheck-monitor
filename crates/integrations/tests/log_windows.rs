//! Window deduplication is bounded and cannot hide events from cancelled collections.
use chrono::{Duration, Utc};
use monitor_core::budget::Budget as Semaphore;
use monitor_core::{
    config::{
        duration::Span,
        resolve::{Job, Selection},
        types::Config,
    },
    model::*,
};
use monitor_integrations::{log_dedup::Dedupe, log_window::Window};
use std::sync::Arc;
fn job() -> Result<Job, Box<dyn std::error::Error>> {
    Config::parse("version=1\n[[targets]]\nname='dev'\nprovider='gcp'\nscope='project'")?
        .resolve(&Selection::default())?
        .jobs
        .into_iter()
        .find(|job| job.check == Check::Logs)
        .ok_or_else(|| "missing job".into())
}
#[test]
fn overlap_deduplicates_only_committed_batches_and_retains_no_payloads()
-> Result<(), Box<dyn std::error::Error>> {
    let mut job = job()?;
    let end = Utc::now();
    job.log_end = Some(end);
    job.continuous = true;
    let dedupe = Dedupe::new(128, Arc::new(Semaphore::new(65536))).ok_or("budget")?;
    for committed in [None, None, Some(end)] {
        job.log_start = committed;
        let mut window = Window::new(&job, "errors", Span(3600), 5, Some(&dedupe));
        window.record("ns/worker", Some("id"), "panic private-customer-value", end)?;
        window.record("ns/worker", Some("id"), "panic private-customer-value", end)?;
        let mut result = CheckResult::failure(
            "dev".into(),
            Check::Logs,
            "revision".into(),
            Coverage::Missing,
        );
        result.operations.clear();
        window.finish(&job, "errors", &mut result, Ok(()), 1);
        let count = result
            .observations
            .iter()
            .filter_map(|obs| match obs.data {
                Data::Log { count, .. } => Some(count),
                _ => None,
            })
            .sum::<u64>();
        assert_eq!(count, if committed.is_none() { 1 } else { 0 });
        assert!(!serde_json::to_string(&result)?.contains("private-customer-value"));
    }
    Ok(())
}
#[test]
fn long_outage_records_the_missing_interval() -> Result<(), Box<dyn std::error::Error>> {
    let mut job = job()?;
    let end = Utc::now();
    job.log_end = Some(end);
    job.log_start = Some(end - Duration::hours(2));
    let window = Window::new(&job, "errors", Span(3600), 5, None);
    let mut result = CheckResult::failure(
        "dev".into(),
        Check::Logs,
        "revision".into(),
        Coverage::Missing,
    );
    result.operations.clear();
    window.finish(&job, "errors", &mut result, Ok(()), 1);
    assert!(result.observations.iter().any(|obs| matches!(
        obs.data,
        Data::LogWindow {
            gap_seconds: 3600,
            complete: true,
            ..
        }
    )));
    assert!(!result.complete());
    Ok(())
}
#[test]
fn exhausted_dedup_budget_is_incomplete_and_fingerprints_remain_bounded()
-> Result<(), Box<dyn std::error::Error>> {
    assert!(Dedupe::new(128, Arc::new(Semaphore::new(1))).is_none());
    let budget = Arc::new(Semaphore::new(65536));
    let dedupe = Dedupe::new(128, budget.clone()).ok_or("budget")?;
    let now = Utc::now();
    for value in 0..10000u64 {
        let mut key = [0; 32];
        key[..8].copy_from_slice(&value.to_le_bytes());
        dedupe.accept(key, now, None);
    }
    assert!(dedupe.len() <= 128);
    drop(dedupe);
    assert_eq!(budget.available_permits(), 65536);
    let mut job = job()?;
    job.continuous = true;
    let window = Window::new(&job, "errors", Span(3600), 5, None);
    let mut result = CheckResult::failure(
        "dev".into(),
        Check::Logs,
        "revision".into(),
        Coverage::Missing,
    );
    result.operations.clear();
    window.finish(&job, "errors", &mut result, Ok(()), 1);
    assert_eq!(result.operations[0].coverage, Coverage::Truncated);
    Ok(())
}

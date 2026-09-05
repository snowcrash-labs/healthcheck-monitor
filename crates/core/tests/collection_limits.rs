//! Collection admission preserves explicit incompleteness before evidence vectors grow.
use monitor_core::{collection_budget::Limit, config::settings::Settings, model::*, state::State};
fn operation() -> Operation {
    Operation {
        id: "inventory".into(),
        coverage: Coverage::Complete,
        observed_at: chrono::Utc::now(),
        records: 1,
        pages: 1,
        attempts: 1,
        required: true,
    }
}
fn observation(name: &str) -> Observation {
    Observation {
        resource: name.into(),
        operation: "inventory".into(),
        observed_at: chrono::Utc::now(),
        expected: Expected::Active,
        data: Data::Condition {
            rule: "available".into(),
            healthy: Some(true),
        },
    }
}
#[test]
fn fitting_exactly_does_not_fabricate_incomplete_coverage() {
    let settings = Settings {
        max_assets: 1,
        ..Default::default()
    };
    let mut budget = Limit::new(&settings);
    let mut result = CheckResult::failure(
        "test".into(),
        Check::Inventory,
        "test".into(),
        Coverage::Missing,
    );
    result.operations.clear();
    assert!(budget.operations(&mut result.operations, [operation()]));
    assert!(budget.observations(&mut result.observations, [observation("resource")]));
    budget.finish(&mut result, true);
    assert!(result.complete());
    assert_eq!(result.operations.len(), 1);
}
#[test]
fn oversized_records_are_rejected_before_accumulation_and_do_not_block_smaller_results() {
    let settings = Settings {
        memory_bytes: 65536,
        concurrency: 1,
        ..Default::default()
    };
    let mut budget = Limit::new(&settings);
    let mut result = CheckResult::failure(
        "test".into(),
        Check::Inventory,
        "test".into(),
        Coverage::Missing,
    );
    result.operations.clear();
    let oversized = observation(&"x".repeat(10000));
    assert!(!budget.observations(&mut result.observations, [oversized]));
    assert!(result.observations.is_empty());
    assert!(budget.observations(&mut result.observations, [observation("small")]));
    assert!(budget.operations(&mut result.operations, [operation()]));
    budget.finish(&mut result, true);
    assert!(!result.complete());
    assert_eq!(result.observations.len(), 1);
    assert!(monitor_core::bounds::result_bytes(&result) < settings.memory_bytes / 2);
}
#[test]
fn repeated_merges_remain_bounded_and_never_claim_complete_truncated_inventories() {
    let settings = Settings {
        memory_bytes: 65536,
        concurrency: 1,
        ..Default::default()
    };
    let mut budget = Limit::new(&settings);
    let mut result = CheckResult::failure(
        "test".into(),
        Check::Inventory,
        "test".into(),
        Coverage::Missing,
    );
    result.operations.clear();
    for index in 0..1000 {
        let mut incoming = result.clone();
        incoming.operations = vec![operation()];
        incoming.observations = vec![observation(&format!("resource-{index}"))];
        budget.merge(&mut result, incoming);
    }
    budget.finish(&mut result, true);
    assert!(!result.complete());
    assert!(monitor_core::bounds::result_bytes(&result) < settings.memory_bytes / 2);
    let mut state = State::new("test".into(), vec![]);
    state
        .snapshot
        .results
        .insert("test/Inventory".into(), result);
    assert!(
        serde_json::to_vec(&state.snapshot).is_ok_and(|bytes| bytes.len() < settings.memory_bytes)
    );
}

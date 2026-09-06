//! Compact reports retain deterministic diagnostics and distinguish endpoint probe intent.
use monitor_core::{model::*, report::markdown, state::State};
#[test]
fn equal_log_counts_sort_by_scope_and_warning_noise_stays_out_of_diagnostics() {
    let now = chrono::Utc::now();
    let mut snapshot = State::new("revision".into(), vec![]).snapshot;
    let mut result = CheckResult::failure(
        "dev".into(),
        Check::Logs,
        "revision".into(),
        Coverage::Complete,
    );
    for (name, signature) in [
        ("worker-b", LogClass::Import),
        ("worker-a", LogClass::Import),
        ("warning-worker", LogClass::Warning),
    ] {
        result.observations.push(Observation {
            context: None,
            resource: format!("dev/logs/{name}"),
            operation: "logs".into(),
            observed_at: now,
            expected: Expected::Active,
            data: Data::Log {
                signature,
                count: 1,
                first_seen: now,
                last_seen: now,
                sampled: true,
            },
        });
    }
    snapshot.results.insert("dev/Logs".into(), result);
    let rendered = markdown(&snapshot);
    assert!(rendered.find("worker-a") < rendered.find("worker-b"));
    assert!(!rendered.contains("warning-worker"));
}

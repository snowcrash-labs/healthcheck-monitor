//! Conservative accounting for retained evidence allocations.
use crate::model::{CheckResult, Coverage, Observation, Operation};
pub fn observation_bytes(observation: &Observation) -> usize {
    // JSON size overestimates most strings; the multiplier reserves collection and comparison copies.
    serde_json::to_vec(observation).map_or(usize::MAX, |bytes| {
        bytes
            .len()
            .saturating_mul(4)
            .saturating_add(std::mem::size_of::<Observation>())
    })
}
pub fn result_bytes(result: &CheckResult) -> usize {
    result.observations.iter().fold(
        result.operations.iter().map(operation_bytes).sum(),
        |sum, o| sum.saturating_add(observation_bytes(o)),
    )
}
pub fn operation_bytes(operation: &Operation) -> usize {
    operation
        .id
        .len()
        .saturating_mul(4)
        .saturating_add(std::mem::size_of::<Operation>() + 256)
}
pub fn truncate(result: &mut CheckResult, bytes: usize, assets: usize) {
    let mut used = result.operations.iter().map(operation_bytes).sum::<usize>();
    let mut retained = 0;
    for observation in result.observations.iter().take(assets) {
        used = used.saturating_add(observation_bytes(observation));
        if used > bytes {
            break;
        }
        retained += 1;
    }
    if retained < result.observations.len() {
        result.observations.truncate(retained);
        for operation in &mut result.operations {
            if operation.coverage == Coverage::Complete {
                operation.coverage = Coverage::Truncated;
            }
        }
    }
}

/// Invalid terminal states and stale timestamps cannot establish complete coverage.
pub fn validate_source(
    result: &mut CheckResult,
    freshness: u64,
    now: chrono::DateTime<chrono::Utc>,
) {
    use crate::model::Data;
    for observation in &result.observations {
        if matches!(
            observation.data,
            Data::Job {
                complete: true,
                failed: true,
                ..
            }
        ) {
            for operation in &mut result.operations {
                if operation.id == observation.operation {
                    operation.coverage = Coverage::Malformed;
                }
            }
        }
        if (now - observation.observed_at).num_seconds() > freshness as i64 {
            for operation in &mut result.operations {
                if operation.id == observation.operation && operation.coverage == Coverage::Complete
                {
                    operation.coverage = Coverage::Stale;
                }
            }
        }
    }
}

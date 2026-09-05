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
        result
            .operations
            .len()
            .saturating_mul(std::mem::size_of::<Operation>() + 1024),
        |sum, o| sum.saturating_add(observation_bytes(o)),
    )
}
pub fn truncate(result: &mut CheckResult, bytes: usize, assets: usize) {
    let mut used = result
        .operations
        .len()
        .saturating_mul(std::mem::size_of::<Operation>() + 1024);
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

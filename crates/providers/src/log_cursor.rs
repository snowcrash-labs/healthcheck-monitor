//! Advance log cursors only after all queried windows have completed.
use monitor_core::model::*;
pub fn complete(result: &CheckResult) -> bool {
    result.observations.iter().any(|obs|matches!(obs.data,Data::LogWindow{complete:true,..})) && result.operations.iter().all(|operation|{
        operation.coverage==Coverage::Complete || operation.coverage==Coverage::Missing && result.observations.iter().any(|obs|operation.id==format!("{}/missing-window",obs.operation) && matches!(obs.data,Data::LogWindow{gap_seconds,complete:true,..}if gap_seconds>0))
    })
}

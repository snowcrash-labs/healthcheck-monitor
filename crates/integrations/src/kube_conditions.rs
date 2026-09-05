//! Readiness keeps unknown conditions distinct from known pressure and network failures.
use super::projection::text;
use serde_json::Value;
pub fn condition(value: &Value, kind: &str) -> Option<bool> {
    value
        .pointer("/status/conditions")
        .and_then(Value::as_array)?
        .iter()
        .find(|value| text(value, &["/type"]) == Some(kind))
        .and_then(|value| text(value, &["/status"]))
        .and_then(|status| match status {
            "True" => Some(true),
            "False" => Some(false),
            _ => None,
        })
}
/// A responding kubelet does not clear known resource pressure or network failure.
pub fn node_ready(value: &Value) -> bool {
    condition(value, "Ready") == Some(true)
        && [
            "MemoryPressure",
            "DiskPressure",
            "PIDPressure",
            "NetworkUnavailable",
        ]
        .iter()
        .all(|kind| condition(value, kind) != Some(true))
}

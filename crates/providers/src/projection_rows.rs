//! Extract list and aggregated-list containers before service projection.
use serde_json::Value;
pub fn rows<'a>(payload: &'a Value, path: &str) -> Vec<&'a Value> {
    match payload.pointer(path) {
        Some(Value::Array(values)) => values.iter().collect(),
        Some(Value::Object(values)) if path == "/items" => values
            .values()
            .flat_map(|v| {
                v.as_object()
                    .into_iter()
                    .flat_map(|m| m.values())
                    .filter_map(Value::as_array)
                    .flatten()
            })
            .collect(),
        Some(value) if value.is_object() => vec![value],
        _ => vec![],
    }
}

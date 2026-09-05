//! Bounded AWS Query XML decoding without external entity expansion.
use super::transport::Error;
use serde_json::{Map, Value};
pub fn decode(bytes: &[u8]) -> Result<Value, Error> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Malformed)?;
    let document = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: 100_000,
            ..Default::default()
        },
    )
    .map_err(|_| Error::Malformed)?;
    node(document.root_element(), 0)
}
fn node(element: roxmltree::Node<'_, '_>, depth: usize) -> Result<Value, Error> {
    if depth > 32 {
        return Err(Error::Limit);
    }
    if !element.children().any(|n| n.is_element()) {
        return Ok(Value::String(element.text().unwrap_or("").into()));
    }
    let mut object = Map::new();
    for child in element.children().filter(|n| n.is_element()) {
        let key = child.tag_name().name();
        let value = node(child, depth + 1)?;
        if let Some(existing) = object.get_mut(key) {
            match existing {
                Value::Array(values) => values.push(value),
                existing => {
                    let old = existing.take();
                    *existing = Value::Array(vec![old, value]);
                }
            }
        } else if matches!(
            key,
            "item"
                | "member"
                | "DBInstance"
                | "DBCluster"
                | "CacheCluster"
                | "ReplicationGroup"
                | "HostedZone"
                | "Bucket"
        ) {
            object.insert(key.into(), Value::Array(vec![value]));
        } else {
            object.insert(key.into(), value);
        }
    }
    Ok(Value::Object(object))
}

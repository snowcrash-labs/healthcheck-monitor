//! Log-console queries contain source identifiers and absolute time bounds only.
use crate::console_links::Link;
use monitor_core::{
    diagnostics::ResourceContext,
    model::{Data, Provider},
};
pub fn links(context: Option<&ResourceContext>, data: &Data) -> Vec<Link> {
    let Some(c) = context.filter(|c| c.provider == Provider::Gcp) else {
        return vec![];
    };
    let (start, end) = match data {
        Data::Log {
            first_seen,
            last_seen,
            ..
        } => (
            *first_seen - chrono::Duration::seconds(60),
            *last_seen + chrono::Duration::seconds(60),
        ),
        Data::LogWindow { start, end, .. } => (*start, *end),
        _ => return vec![],
    };
    let quote = |value: &str| serde_json::to_string(value).ok();
    let mut filters = vec![
        format!("timestamp >= \"{}\"", start.to_rfc3339()),
        format!("timestamp <= \"{}\"", end.to_rfc3339()),
    ];
    for (field, value) in [
        ("namespace_name", &c.namespace),
        ("cluster_name", &c.cluster),
        ("container_name", &c.container),
    ] {
        if let Some(value) = value.as_deref().and_then(quote) {
            filters.push(format!("resource.labels.{field}={value}"));
        }
    }
    if c.namespace.is_some()
        && let Some(name) = c.name.as_deref().and_then(quote)
    {
        filters.push(format!("resource.labels.pod_name={name}"));
    }
    let encoded = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    vec![Link {
        label: if c.namespace.is_some() {
            "View source logs"
        } else {
            "View project logs in this time window"
        }
        .into(),
        url: format!(
            "https://console.cloud.google.com/logs/query;query={}?project={}",
            encoded(&filters.join("\n")),
            encoded(&c.scope)
        ),
    }]
}

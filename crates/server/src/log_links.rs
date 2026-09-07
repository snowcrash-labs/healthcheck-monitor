//! Incident log links use absolute observation windows and explicit monitored-resource types.
use crate::console_links::{Link, encoded};
use chrono::{DateTime, Utc};
use monitor_core::{
    diagnostics::ResourceContext,
    model::{Data, Provider},
};

pub fn links(context: Option<&ResourceContext>, data: &Data) -> Vec<Link> {
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
    window(context, start, end)
}
pub fn incident(context: Option<&ResourceContext>, at: DateTime<Utc>) -> Vec<Link> {
    window(
        context,
        at - chrono::Duration::minutes(5),
        at + chrono::Duration::minutes(5),
    )
}
fn window(
    context: Option<&ResourceContext>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Vec<Link> {
    let Some(c) = context.filter(|c| c.provider == Provider::Gcp) else {
        return vec![];
    };
    if c.scope.is_empty() || c.scope.len() > 128 || c.scope.chars().any(char::is_control) {
        return vec![];
    }
    let quote = |value: &str| serde_json::to_string(value).ok();
    let mut filters = vec![
        format!("timestamp >= \"{}\"", start.to_rfc3339()),
        format!("timestamp < \"{}\"", end.to_rfc3339()),
    ];
    let mut scoped = false;
    if c.namespace.is_some() {
        filters.push("resource.type=\"k8s_container\"".into());
        for (field, value) in [
            ("namespace_name", &c.namespace),
            ("cluster_name", &c.cluster),
            ("container_name", &c.container),
        ] {
            if let Some(value) = value.as_deref().and_then(quote) {
                filters.push(format!("resource.labels.{field}={value}"));
            }
        }
        if c.service == "pods" || c.service == "logs" {
            if let Some(name) = c.name.as_deref().and_then(quote) {
                filters.push(format!("resource.labels.pod_name={name}"));
            }
            scoped = c.name.is_some() && c.cluster.is_some();
        }
    } else if matches!(c.service.as_str(), "cloud-run" | "run") {
        filters.push("resource.type=\"cloud_run_revision\"".into());
        if let Some(name) = c
            .name
            .as_deref()
            .or_else(|| c.native_id.rsplit('/').next())
            .and_then(quote)
        {
            filters.push(format!("resource.labels.service_name={name}"));
            scoped = true;
        }
    } else if matches!(c.service.as_str(), "sql" | "sql-instances") {
        filters.push("resource.type=\"cloudsql_database\"".into());
        if let Some(name) = c.name.as_deref().or_else(|| c.native_id.rsplit('/').next()) {
            if let Some(id) = quote(&format!("{}:{name}", c.scope)) {
                filters.push(format!("resource.labels.database_id={id}"));
                scoped = true;
            }
        }
    }
    vec![Link {
        label: if scoped {
            "View incident logs"
        } else if c.namespace.is_some() && c.cluster.is_some() {
            "View namespace logs in this window"
        } else {
            "View project logs in this window"
        }
        .into(),
        url: format!(
            "https://console.cloud.google.com/logs/query;query={}?project={}",
            encoded(&filters.join("\n")),
            encoded(&c.scope)
        ),
    }]
}

//! Bounded follow-up reads derived only from discovered resource identifiers.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::Provider};
use monitor_integrations::projection::text;
use serde_json::{Value, json};
pub fn followups(job: &Job, parent: &Endpoint, row: &Value) -> Vec<Endpoint> {
    let family = parent.id.split('/').next().unwrap_or("");
    let name = if job.target.provider == Provider::Azure {
        text(row, &["/id"])
    } else {
        None
    }
    .or_else(|| {
        text(
            row,
            &[
                "/name",
                "/id",
                "/Name",
                "/KeyId",
                "/TableName",
                "/repositoryName",
                "/TargetGroupArn",
                "/Id",
                "/CertificateArn",
                "/BackupVaultName",
                "/FunctionName",
            ],
        )
        .or_else(|| row.as_str())
    });
    let Some(name) = name else { return vec![] };
    if !job.target.resources.is_empty()
        && !job
            .target
            .resources
            .iter()
            .any(|selector| name.contains(selector) || parent.id.contains(selector))
    {
        return vec![];
    }
    if !(monitor_core::config::validate::identifier(name)
        || job.target.provider == Provider::Azure
            && crate::azure_metric_discovery::valid_resource(job, name))
    {
        return vec![];
    }
    match job.target.provider {
        Provider::Gcp => gcp(job, parent, family, name, row),
        Provider::Aws => crate::aws_details::followups(job, parent, family, name),
        Provider::Azure => crate::azure_details::followups(job, parent, family, name),
        _ => vec![],
    }
}
fn gcp(job: &Job, parent: &Endpoint, family: &str, name: &str, row: &Value) -> Vec<Endpoint> {
    let p = &job.target.scope;
    let mut out = Vec::new();
    let paths: Vec<(String, String, &str)> = match family {
        "sql" => vec![
            (
                format!("sql-backups/{name}"),
                format!(
                    "sqladmin.googleapis.com/sql/v1beta4/projects/{p}/instances/{name}/backupRuns?maxResults=5"
                ),
                "/items",
            ),
            (
                format!("sql-operations/{name}"),
                format!(
                    "sqladmin.googleapis.com/sql/v1beta4/projects/{p}/operations?instance={name}&maxResults=20"
                ),
                "/items",
            ),
        ],
        "dns-zones" => vec![(
            format!("dns-records/{name}"),
            format!("dns.googleapis.com/dns/v1/projects/{p}/managedZones/{name}/rrsets"),
            "/rrsets",
        )],
        "kms-keyrings" => vec![(
            format!("kms-keys/{name}"),
            format!("cloudkms.googleapis.com/v1/{name}/cryptoKeys"),
            "/cryptoKeys",
        )],
        "kms-keys" => vec![(
            format!("kms-versions/{name}"),
            format!("cloudkms.googleapis.com/v1/{name}/cryptoKeyVersions"),
            "/cryptoKeyVersions",
        )],
        "secrets" => vec![(
            format!("secret-versions/{name}"),
            format!("secretmanager.googleapis.com/v1/{name}/versions"),
            "/versions",
        )],
        "buckets" => vec![(
            format!("bucket-config/{name}"),
            format!("storage.googleapis.com/storage/v1/b/{name}"),
            "",
        )],
        "artifact-repositories" => vec![(
            format!("registry-images/{name}"),
            format!("artifactregistry.googleapis.com/v1/{name}/dockerImages"),
            "/dockerImages",
        )],
        "slo-services" => vec![(
            format!("slo-objectives/{name}"),
            format!("monitoring.googleapis.com/v3/{name}/serviceLevelObjectives"),
            "/serviceLevelObjectives",
        )],
        _ => vec![],
    };
    for (id, url, items) in paths {
        out.push(Endpoint::get(id, format!("https://{url}"), items));
    }
    if family == "backend-services"
        && let Some(backends) = row.get("backends").and_then(Value::as_array)
    {
        for (i, backend) in backends.iter().take(100).enumerate() {
            if let Some(group) = text(backend, &["/group"]) {
                let mut endpoint = Endpoint::get(
                    format!("backend-health/{name}/{i}"),
                    format!("{}/{name}/getHealth", parent.url),
                    "/healthStatus",
                );
                endpoint.body = Some(json!({"group":group}));
                out.push(endpoint);
            }
        }
    }
    out
}

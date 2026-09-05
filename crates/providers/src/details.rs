//! Bounded follow-up reads derived only from discovered resource identifiers.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::Provider};
use monitor_integrations::projection::text;
use serde_json::{Value, json};
pub fn followups(job: &Job, parent: &Endpoint, row: &Value) -> Vec<Endpoint> {
    if parent.id.starts_with("pipeline-state/") {
        return crate::aws_pipeline::followups(job, parent, row);
    }
    if parent.id.starts_with("kv-") {
        return crate::key_vault::followups(job, parent, row);
    }
    if parent.id.starts_with("registry-images/") {
        return crate::registry_manifests::followups(job, parent, row);
    }
    let family = parent.id.split('/').next().unwrap_or("");
    let name = if family == "route53" {
        text(row, &["/Id"])
    } else if job.target.provider == Provider::Azure {
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
                "/ServiceCode",
                "/TableName",
                "/repositoryName",
                "/TargetGroupArn",
                "/Id",
                "/CertificateArn",
                "/BackupVaultName",
                "/FunctionName",
                "/serviceArn",
                "/taskArn",
            ],
        )
        .or_else(|| row.as_str())
    });
    let Some(name) = name else { return vec![] };
    if !matches!(
        family,
        "ecs-clusters"
            | "ecs-services"
            | "ecs-tasks"
            | "artifact-repositories"
            | "ecr"
            | "key-vaults"
            | "quota-services"
    ) && !job.target.resources.is_empty()
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
        Provider::Aws => crate::aws_details::followups(job, parent, family, name, row),
        Provider::Azure => {
            let mut details = crate::azure_details::followups(job, parent, family, name);
            if family == "key-vaults" {
                details.extend(crate::key_vault::followups(job, parent, row));
            }
            details
        }
        _ => vec![],
    }
}
fn gcp(job: &Job, parent: &Endpoint, family: &str, name: &str, row: &Value) -> Vec<Endpoint> {
    let p = &job.target.scope;
    let mut out = Vec::new();
    if family == "cloud-run" {
        let mut revisions: std::collections::BTreeSet<_> = row
            .get("trafficStatuses")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|traffic| {
                monitor_integrations::projection::number(traffic, &["/percent"])
                    .is_some_and(|percent| percent > 0.0)
            })
            .filter_map(|traffic| text(traffic, &["/revision"]))
            .map(String::from)
            .collect();
        if revisions.is_empty()
            && let Some(revision) = text(row, &["/latestReadyRevision"])
        {
            revisions.insert(revision.into());
        }
        for revision in revisions.into_iter().take(job.settings.max_assets) {
            let revision = if revision.starts_with("projects/") {
                revision
            } else {
                format!("{name}/revisions/{revision}")
            };
            if !revision.starts_with(&format!("{name}/revisions/"))
                || !monitor_core::config::validate::identifier(&revision)
            {
                continue;
            }
            let Some((_, location)) = revision.split_once("/locations/") else {
                continue;
            };
            out.push(Endpoint::get(
                format!("cloud-run-revision/{revision}"),
                format!("https://run.googleapis.com/v2/projects/{p}/locations/{location}"),
                "",
            ));
        }
    }
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

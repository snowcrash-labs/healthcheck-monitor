//! Bounded operational ARM reads derived from full resource identities.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::Check};
pub fn followups(job: &Job, _parent: &Endpoint, family: &str, name: &str) -> Vec<Endpoint> {
    if !crate::azure_metric_discovery::valid_resource(job, name) {
        return vec![];
    }
    let specs: &[(&str, &str, &str, &str)] = match family {
        "service-bus" => &[("service-bus-queues", "queues", "2024-01-01", "/value")],
        "event-hubs" => &[("event-hub-details", "eventhubs", "2024-01-01", "/value")],
        "sql" => &[("sql-databases", "databases", "2023-08-01", "/value")],
        "sql-databases" => &[(
            "sql-backup-retention",
            "backupShortTermRetentionPolicies/default",
            "2023-08-01",
            "",
        )],
        "backup-vaults" => &[(
            "backup-protected-items",
            "backupProtectedItems",
            "2024-04-01",
            "/value",
        )],
        "vms" => &[("vm-instance-view", "instanceView", "2024-11-01", "")],
        "vm-scale-sets" => &[("vmss-instances", "virtualMachines", "2024-07-01", "/value")],
        "aks" => &[("aks-pools", "agentPools", "2025-01-01", "/value")],
        "container-apps" => &[("container-revisions", "revisions", "2024-03-01", "/value")],
        "app-service" => &[
            ("app-instances", "instances", "2024-04-01", "/value"),
            ("app-deployments", "deployments", "2024-04-01", "/value"),
        ],
        "front-door" => &[
            (
                "front-door-endpoints",
                "afdEndpoints",
                "2024-09-01",
                "/value",
            ),
            ("origin-groups", "originGroups", "2024-09-01", "/value"),
        ],
        "origin-groups" => &[("front-door-origins", "origins", "2024-09-01", "/value")],
        "dns-zones" => &[("dns-records", "recordsets", "2018-05-01", "/value")],
        "storage" => &[("blob-service", "blobServices/default", "2024-01-01", "")],
        _ => &[],
    };
    let mut out: Vec<_> = specs
        .iter()
        .map(|(id, path, api, items)| {
            Endpoint::get(
                format!("{id}/{name}"),
                format!(
                    "https://management.azure.com{name}/{path}?api-version={api}{}",
                    if *id == "vmss-instances" {
                        "&$expand=instanceView"
                    } else {
                        ""
                    }
                ),
                items,
            )
        })
        .collect();
    if family == "application-gateways" {
        let mut endpoint = Endpoint::get(
            format!("gateway-health/{name}"),
            format!("https://management.azure.com{name}/backendhealth?api-version=2024-05-01"),
            "",
        );
        endpoint.body = Some(serde_json::json!({}));
        out.push(endpoint);
    }
    if matches!(job.check, Check::Inventory | Check::Managed)
        && matches!(
            family,
            "vms"
                | "vm-scale-sets"
                | "aks"
                | "container-apps"
                | "app-service"
                | "load-balancers"
                | "application-gateways"
                | "sql-databases"
                | "postgresql"
                | "redis"
                | "cosmos-db"
                | "storage"
                | "service-bus"
                | "event-hubs"
                | "event-grid"
                | "key-vaults"
        )
    {
        out.push(Endpoint::get(format!("resource-availability/{name}"),format!("https://management.azure.com{name}/providers/Microsoft.ResourceHealth/availabilityStatuses/current?api-version=2025-05-01"),""));
        out.push(Endpoint::get(format!("diagnostics/{name}"),format!("https://management.azure.com{name}/providers/Microsoft.Insights/diagnosticSettings?api-version=2021-05-01-preview"),"/value"));
    }
    out
}

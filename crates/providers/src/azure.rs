//! Azure ARM health and metadata reads across explicitly selected subscriptions.
use crate::{
    auth::Auth,
    common::{self, Endpoint},
};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::transport::Http;
use tokio_util::sync::CancellationToken;

pub fn endpoints(job: &Job) -> Vec<Endpoint> {
    let root = format!(
        "https://management.azure.com/subscriptions/{}",
        job.target.scope
    );
    if job.check == Check::Preflight {
        return vec![Endpoint::get(
            "subscription-scope",
            format!("{root}?api-version=2022-12-01"),
            "",
        )];
    }
    let mut out = Vec::new();
    for (name, namespace, api) in CATALOG {
        if job.check == Check::Alerts
            && !matches!(
                *name,
                "alerts" | "activity-log-alerts" | "metric-alerts" | "service-health"
            )
        {
            continue;
        }
        if job.check == Check::Releases
            && !matches!(*name, "container-apps" | "app-service" | "registries")
        {
            continue;
        }
        out.push(Endpoint::get(
            *name,
            format!("{root}/providers/{namespace}?api-version={api}"),
            "/value",
        ));
    }
    out
}
pub async fn collect(
    http: &Http,
    auth: &Auth,
    job: &Job,
    cancel: &CancellationToken,
) -> CheckResult {
    if job.check == Check::Metrics || job.check == Check::Queues {
        return crate::metrics::azure(http, auth, job, cancel).await;
    }
    common::collect(http, auth, job, endpoints(job), cancel).await
}
const CATALOG: &[(&str, &str, &str)] = &[
    (
        "resource-groups",
        "Microsoft.Resources/resourceGroups",
        "2022-09-01",
    ),
    ("vms", "Microsoft.Compute/virtualMachines", "2024-11-01"),
    (
        "vm-scale-sets",
        "Microsoft.Compute/virtualMachineScaleSets",
        "2024-11-01",
    ),
    ("disks", "Microsoft.Compute/disks", "2024-03-02"),
    (
        "aks",
        "Microsoft.ContainerService/managedClusters",
        "2025-01-01",
    ),
    (
        "container-apps",
        "Microsoft.App/containerApps",
        "2024-03-01",
    ),
    ("app-service", "Microsoft.Web/sites", "2024-04-01"),
    (
        "load-balancers",
        "Microsoft.Network/loadBalancers",
        "2024-05-01",
    ),
    (
        "application-gateways",
        "Microsoft.Network/applicationGateways",
        "2024-05-01",
    ),
    ("front-door", "Microsoft.Cdn/profiles", "2024-09-01"),
    ("dns-zones", "Microsoft.Network/dnszones", "2018-05-01"),
    (
        "network-security",
        "Microsoft.Network/networkSecurityGroups",
        "2024-05-01",
    ),
    ("sql", "Microsoft.Sql/servers", "2023-08-01"),
    (
        "postgresql",
        "Microsoft.DBforPostgreSQL/flexibleServers",
        "2024-08-01",
    ),
    ("redis", "Microsoft.Cache/redis", "2024-11-01"),
    (
        "cosmos-db",
        "Microsoft.DocumentDB/databaseAccounts",
        "2024-11-15",
    ),
    ("storage", "Microsoft.Storage/storageAccounts", "2024-01-01"),
    (
        "service-bus",
        "Microsoft.ServiceBus/namespaces",
        "2024-01-01",
    ),
    ("event-hubs", "Microsoft.EventHub/namespaces", "2024-01-01"),
    ("event-grid", "Microsoft.EventGrid/topics", "2025-02-15"),
    (
        "registries",
        "Microsoft.ContainerRegistry/registries",
        "2023-07-01",
    ),
    ("key-vaults", "Microsoft.KeyVault/vaults", "2023-07-01"),
    (
        "backup-vaults",
        "Microsoft.RecoveryServices/vaults",
        "2024-04-01",
    ),
    ("alerts", "Microsoft.AlertsManagement/alerts", "2019-03-01"),
    (
        "metric-alerts",
        "Microsoft.Insights/metricAlerts",
        "2018-03-01",
    ),
    (
        "activity-log-alerts",
        "Microsoft.Insights/activityLogAlerts",
        "2020-10-01",
    ),
    (
        "service-health",
        "Microsoft.ResourceHealth/events",
        "2024-02-01",
    ),
];

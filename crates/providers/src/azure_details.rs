//! Follow-up reads derived from selected resource identities.
use crate::common::Endpoint;
use monitor_core::config::resolve::Job;
pub fn followups(job: &Job, _parent: &Endpoint, family: &str, name: &str) -> Vec<Endpoint> {
    if !name.starts_with(&format!("/subscriptions/{}/", job.target.scope)) {
        return vec![];
    }
    let (id, path, version) = match family {
        "service-bus" => ("service-bus-queues", "queues", "2024-01-01"),
        "event-hubs" => ("event-hub-details", "eventhubs", "2024-01-01"),
        "sql" => ("sql-databases", "databases", "2023-08-01"),
        "backup-vaults" => (
            "backup-protected-items",
            "backupProtectedItems",
            "2024-04-01",
        ),
        "vms" => ("vm-instance-view", "instanceView", "2024-11-01"),
        _ => return vec![],
    };
    vec![Endpoint::get(
        format!("{id}/{name}"),
        format!("https://management.azure.com{name}/{path}?api-version={version}"),
        if family == "vms" { "" } else { "/value" },
    )]
}

//! Azure runtime, dependency, recovery, and quota evidence without configuration payloads.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::projection::{boolean, number, observation, state, text, timestamp};
use serde_json::Value;
pub fn project(job: &Job, endpoint: &Endpoint, value: &Value) -> Option<Vec<Observation>> {
    let family = endpoint.id.split('/').next()?;
    let id = &endpoint.id;
    let name = text(value, &["/id", "/name", "/name/value"]).unwrap_or("status");
    if !job.target.resources.is_empty()
        && !job
            .target
            .resources
            .iter()
            .any(|selector| name.contains(selector) || id.contains(selector))
    {
        return Some(vec![]);
    }
    if family == "gateway-health" {
        return Some(gateway(job, endpoint, value));
    }
    let data = match family {
        "vm-instance-view" | "vmss-instances" => {
            let statuses = value
                .pointer("/statuses")
                .or_else(|| value.pointer("/properties/instanceView/statuses"))
                .and_then(Value::as_array);
            let power = statuses
                .into_iter()
                .flatten()
                .filter_map(|status| text(status, &["/code"]))
                .find_map(|code| code.strip_prefix("PowerState/"));
            Data::Service {
                state: state(power),
                replicas: None,
                backup_enabled: None,
                encrypted: None,
            }
        }
        "resource-availability" => Data::Condition {
            rule: "resource-unavailable".into(),
            healthy: text(value, &["/properties/availabilityState"]).and_then(
                |state| match state {
                    "Available" => Some(true),
                    "Unavailable" | "Degraded" => Some(false),
                    _ => None,
                },
            ),
        },
        "container-revisions" => Data::Service {
            state: if boolean(value, &["/properties/active"]) == Some(false) {
                ServiceState::Stopped
            } else {
                match text(value, &["/properties/healthState"]) {
                    Some("Healthy") => ServiceState::Ready,
                    Some("Unhealthy") => ServiceState::Failed,
                    _ => state(text(value, &["/properties/runningState"])),
                }
            },
            replicas: number(value, &["/properties/replicas"]).map(|value| value as u32),
            backup_enabled: None,
            encrypted: None,
        },
        "aks-pools" => Data::Service {
            state: state(text(
                value,
                &[
                    "/properties/powerState/code",
                    "/properties/provisioningState",
                ],
            )),
            replicas: number(value, &["/properties/count"]).map(|value| value as u32),
            backup_enabled: None,
            encrypted: None,
        },
        "service-bus-queues" => {
            let mut out = Vec::new();
            for (kind, path) in [
                (
                    "active-messages",
                    "/properties/countDetails/activeMessageCount",
                ),
                (
                    "dead-letter-messages",
                    "/properties/countDetails/deadLetterMessageCount",
                ),
                (
                    "delivery-failures",
                    "/properties/countDetails/transferDeadLetterMessageCount",
                ),
            ] {
                if let Some(value) = number(value, &[path]) {
                    out.push(observation(
                        job,
                        id,
                        &format!("{name}/{kind}"),
                        Data::Metric {
                            name: kind.into(),
                            value,
                            capacity: None,
                            warning: (kind != "active-messages").then_some(1.0),
                            error: None,
                            window_seconds: 0,
                        },
                    ));
                }
            }
            return Some(out);
        }
        "quotas" => Data::Metric {
            name: text(value, &["/name/value"]).unwrap_or("quota").into(),
            value: number(value, &["/currentValue"])?,
            capacity: number(value, &["/limit"]).filter(|limit| *limit > 0.0),
            warning: None,
            error: None,
            window_seconds: 0,
        },
        "sql-backup-retention" => Data::Recovery {
            state: ServiceState::Unknown,
            enabled: number(value, &["/properties/retentionDays"]).map(|days| days > 0.0),
            last_attempt: None,
            last_success: None,
            retention_days: number(value, &["/properties/retentionDays"]).map(|days| days as u32),
            point_in_time: None,
            geo_redundant: None,
        },
        "backup-protected-items" => {
            let status = match text(value, &["/properties/lastBackupStatus"]) {
                Some("Completed") => ServiceState::Ready,
                other => state(other),
            };
            let at = timestamp(value, &["/properties/lastBackupTime"]);
            Data::Recovery {
                state: status,
                enabled: text(value, &["/properties/protectionState"])
                    .map(|state| state == "Protected"),
                last_attempt: at,
                last_success: if status == ServiceState::Ready {
                    at
                } else {
                    None
                },
                retention_days: None,
                point_in_time: None,
                geo_redundant: None,
            }
        }
        "blob-service" => Data::Recovery {
            state: ServiceState::Unknown,
            enabled: None,
            last_attempt: None,
            last_success: None,
            retention_days: number(value, &["/properties/deleteRetentionPolicy/days"])
                .map(|days| days as u32),
            point_in_time: boolean(value, &["/properties/restorePolicy/enabled"]),
            geo_redundant: None,
        },
        "diagnostics" => Data::Inventory {
            family: format!(
                "diagnostics/logs-{}",
                value
                    .pointer("/properties/logs")
                    .and_then(Value::as_array)
                    .is_some_and(|logs| logs
                        .iter()
                        .any(|log| boolean(log, &["/enabled"]) == Some(true)))
            ),
            supported: true,
        },
        _ => return None,
    };
    let mut obs = observation(job, id, name, data);
    if family == "container-revisions"
        && (boolean(value, &["/properties/active"]) == Some(false)
            || number(value, &["/properties/replicas"]) == Some(0.0)
                && !matches!(
                    obs.data,
                    Data::Service {
                        state: ServiceState::Failed,
                        ..
                    }
                ))
    {
        obs.expected = Expected::ScaleToZero;
    }
    Some(vec![obs])
}
fn gateway(job: &Job, endpoint: &Endpoint, value: &Value) -> Vec<Observation> {
    let mut out = Vec::new();
    for pool in value
        .get("backendAddressPools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(job.settings.max_assets)
    {
        let pool_id = text(pool, &["/backendAddressPool/id"]).unwrap_or("pool");
        for settings in pool
            .get("backendHttpSettingsCollection")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for server in settings
                .get("servers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if out.len() > job.settings.max_assets {
                    return out;
                }
                let address = text(server, &["/address"]).unwrap_or("unknown");
                out.push(observation(
                    job,
                    &endpoint.id,
                    &format!("{pool_id}/{address}"),
                    Data::Condition {
                        rule: "gateway-backend-unhealthy".into(),
                        healthy: text(server, &["/health"]).and_then(|health| match health {
                            "Healthy" => Some(true),
                            "Unhealthy" => Some(false),
                            _ => None,
                        }),
                    },
                ));
            }
        }
    }
    out
}

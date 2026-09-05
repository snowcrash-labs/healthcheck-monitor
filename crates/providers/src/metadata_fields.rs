//! Allowlisted network, key, and maintenance metadata never retains unrestricted provider objects.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::projection::{boolean, identity, number, observation, text, timestamp};
use serde_json::Value;
/// Report metadata caps instead of silently treating a partial policy as complete.
pub fn validate(
    job: &Job,
    endpoint: &Endpoint,
    value: &Value,
) -> Result<(), monitor_integrations::transport::Error> {
    let family = endpoint.id.split('/').next().unwrap_or("");
    let rules = value.get("allowed").and_then(Value::as_array);
    let truncated = family == "firewalls"
        && (rules.is_some_and(|rules| rules.len() > 32)
            || rules
                .into_iter()
                .flatten()
                .filter_map(|rule| rule.get("ports").and_then(Value::as_array))
                .map(Vec::len)
                .sum::<usize>()
                > 128)
        || family == "network-security"
            && value
                .pointer("/properties/securityRules")
                .and_then(Value::as_array)
                .is_some_and(|rules| rules.len() > job.settings.max_assets);
    if truncated {
        Err(monitor_integrations::transport::Error::Limit)
    } else {
        Ok(())
    }
}
fn network_token(value: &str) -> String {
    value
        .chars()
        .filter(|value| value.is_ascii_alphanumeric() || "-*,".contains(*value))
        .take(128)
        .collect()
}
pub fn project(job: &Job, endpoint: &Endpoint, value: &Value) -> Vec<Observation> {
    let family = endpoint.id.split('/').next().unwrap_or("");
    let name = text(
        value,
        &[
            "/name",
            "/id",
            "/KeyId",
            "/DBInstanceIdentifier",
            "/DBClusterIdentifier",
            "/CacheClusterId",
            "/ReplicationGroupId",
            "/groupId",
        ],
    )
    .unwrap_or("metadata");
    let mut out = Vec::new();
    if matches!(
        family,
        "kms-keys" | "kms-detail" | "kms-versions" | "secret-versions"
    ) {
        let state = text(value, &["/primary/state", "/state", "/KeyState"]);
        let enabled = boolean(value, &["/Enabled", "/attributes/enabled"]).or_else(|| {
            state.and_then(|state| match state {
                "ENABLED" | "Enabled" => Some(true),
                "DISABLED" | "Disabled" | "DESTROYED" | "PendingDeletion" => Some(false),
                _ => None,
            })
        });
        let purpose = text(value, &["/purpose", "/KeyUsage"])
            .filter(|purpose| {
                matches!(
                    *purpose,
                    "ENCRYPT_DECRYPT"
                        | "ASYMMETRIC_SIGN"
                        | "ASYMMETRIC_DECRYPT"
                        | "MAC"
                        | "SIGN_VERIFY"
                        | "GENERATE_VERIFY_MAC"
                )
            })
            .map(String::from);
        out.push(observation(
            job,
            &endpoint.id,
            &format!("{name}/key-metadata"),
            Data::KeyMetadata {
                enabled,
                purpose,
                rotates_at: timestamp(value, &["/nextRotationTime"]),
                expires_at: timestamp(value, &["/ValidTo", "/attributes/exp"]),
            },
        ));
    }
    if matches!(
        family,
        "sql"
            | "redis"
            | "valkey"
            | "rds"
            | "aurora"
            | "elasticache"
            | "replication-groups"
            | "postgresql"
    ) {
        let window = text(
            value,
            &[
                "/PreferredMaintenanceWindow",
                "/maintenanceWindow/startTime",
                "/maintenanceSchedule/startTime",
            ],
        )
        .map(identity)
        .or_else(|| {
            number(
                value,
                &[
                    "/settings/maintenanceWindow/day",
                    "/properties/maintenanceWindow/dayOfWeek",
                ],
            )
            .map(|day| {
                format!(
                    "day-{day}/hour-{}",
                    number(
                        value,
                        &[
                            "/settings/maintenanceWindow/hour",
                            "/properties/maintenanceWindow/startHour"
                        ]
                    )
                    .unwrap_or(0.0)
                )
            })
        });
        let pending = value
            .get("PendingModifiedValues")
            .is_some_and(|value| value.as_object().is_some_and(|value| !value.is_empty()))
            || value.get("maintenanceSchedule").is_some();
        out.push(observation(
            job,
            &endpoint.id,
            &format!("{name}/maintenance"),
            Data::Maintenance { window, pending },
        ));
    }
    if family == "firewalls" {
        let rules = value.get("allowed").and_then(Value::as_array);
        let protocols = rules
            .into_iter()
            .flatten()
            .filter_map(|rule| text(rule, &["/IPProtocol"]))
            .take(32)
            .map(network_token)
            .collect();
        let ports = rules
            .into_iter()
            .flatten()
            .filter_map(|rule| rule.get("ports").and_then(Value::as_array))
            .flatten()
            .filter_map(Value::as_str)
            .take(128)
            .map(network_token)
            .collect();
        let public = value
            .get("sourceRanges")
            .and_then(Value::as_array)
            .map(|ranges| {
                ranges
                    .iter()
                    .any(|range| matches!(range.as_str(), Some("0.0.0.0/0" | "::/0")))
            });
        out.push(observation(
            job,
            &endpoint.id,
            &format!("{name}/network-policy"),
            Data::NetworkPolicy {
                direction: text(value, &["/direction"])
                    .filter(|direction| matches!(*direction, "INGRESS" | "EGRESS"))
                    .map(String::from),
                allows_public: public.map(|public| {
                    public && rules.is_some() && boolean(value, &["/disabled"]) != Some(true)
                }),
                protocols,
                ports,
                priority: number(value, &["/priority"]).map(|priority| priority as u32),
            },
        ));
    }
    if family == "network-security" {
        for rule in value
            .pointer("/properties/securityRules")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(job.settings.max_assets)
        {
            let source = text(rule, &["/properties/sourceAddressPrefix"]);
            let access = text(rule, &["/properties/access"]);
            let rule_name = text(rule, &["/name"]).unwrap_or("rule");
            out.push(observation(
                job,
                &endpoint.id,
                &format!("{name}/{rule_name}"),
                Data::NetworkPolicy {
                    direction: text(rule, &["/properties/direction"])
                        .filter(|value| matches!(*value, "Inbound" | "Outbound"))
                        .map(String::from),
                    allows_public: source.map(|source| {
                        matches!(source, "*" | "Internet" | "0.0.0.0/0" | "::/0")
                            && access == Some("Allow")
                    }),
                    protocols: text(rule, &["/properties/protocol"])
                        .map(network_token)
                        .into_iter()
                        .collect(),
                    ports: text(rule, &["/properties/destinationPortRange"])
                        .map(network_token)
                        .into_iter()
                        .collect(),
                    priority: number(rule, &["/properties/priority"])
                        .map(|priority| priority as u32),
                },
            ));
        }
    }
    out
}

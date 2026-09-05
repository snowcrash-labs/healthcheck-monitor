//! Discard unallowlisted provider data at the collection boundary.
use chrono::{DateTime, Utc};
use monitor_core::{config::resolve::Job, model::*};
use serde_json::Value;
pub fn text<'a>(value: &'a Value, paths: &[&str]) -> Option<&'a str> {
    paths
        .iter()
        .find_map(|p| value.pointer(p).and_then(Value::as_str))
}
pub fn number(value: &Value, paths: &[&str]) -> Option<f64> {
    paths.iter().find_map(|p| {
        value.pointer(p).and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
    })
}
pub fn boolean(value: &Value, paths: &[&str]) -> Option<bool> {
    paths
        .iter()
        .find_map(|p| value.pointer(p).and_then(Value::as_bool))
}
pub fn timestamp(value: &Value, paths: &[&str]) -> Option<DateTime<Utc>> {
    text(value, paths)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.to_utc())
}
pub fn identity(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "-_./:@".contains(*c))
        .take(512)
        .collect()
}
pub fn state(value: Option<&str>) -> ServiceState {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some(
            "running" | "runnable" | "ready" | "available" | "active" | "succeeded" | "success"
            | "healthy" | "enabled" | "ok",
        ) => ServiceState::Ready,
        Some("stopped" | "terminated" | "suspended" | "disabled" | "deallocated" | "shutdown") => {
            ServiceState::Stopped
        }
        Some(
            "failed" | "failure" | "error" | "unhealthy" | "impaired" | "degraded" | "unavailable"
            | "alarm",
        ) => ServiceState::Failed,
        Some(
            "pending" | "creating" | "updating" | "starting" | "provisioning" | "queued"
            | "working" | "in_progress",
        ) => ServiceState::Starting,
        _ => ServiceState::Unknown,
    }
}
pub fn observation(job: &Job, operation: &str, name: &str, data: Data) -> Observation {
    Observation {
        resource: format!("{}/{}/{}", job.target.name, operation, identity(name)),
        operation: operation.into(),
        observed_at: Utc::now(),
        expected: job.target.expected,
        data,
    }
}
pub fn service(job: &Job, operation: &str, value: &Value) -> Option<Observation> {
    let name = text(
        value,
        &[
            "/name",
            "/id",
            "/arn",
            "/Arn",
            "/DBInstanceIdentifier",
            "/CacheClusterId",
            "/TableName",
            "/FunctionName",
            "/RepositoryName",
            "/QueueUrl",
            "/InstanceId",
            "/AutoScalingGroupName",
            "/VolumeId",
            "/LoadBalancerArn",
        ],
    )?;
    if !job.target.resources.is_empty() && !job.target.resources.iter().any(|s| name.contains(s)) {
        return None;
    }
    let state = state(text(
        value,
        &[
            "/state",
            "/status",
            "/State/Name",
            "/State",
            "/Status",
            "/DBInstanceStatus",
            "/CacheClusterStatus",
            "/TableStatus",
            "/properties/provisioningState",
            "/properties/state",
            "/properties/status",
            "/healthStatus",
        ],
    ));
    Some(observation(
        job,
        operation,
        name,
        Data::Service {
            state,
            replicas: number(
                value,
                &[
                    "/replicas",
                    "/desiredCount",
                    "/DesiredCapacity",
                    "/properties/replicas",
                ],
            )
            .map(|v| v as u32),
            backup_enabled: boolean(
                value,
                &[
                    "/settings/backupConfiguration/enabled",
                    "/properties/backup/geoRedundantBackup",
                ],
            )
            .or_else(|| {
                number(
                    value,
                    &[
                        "/BackupRetentionPeriod",
                        "/properties/backup/backupRetentionDays",
                    ],
                )
                .map(|n| n > 0.0)
            }),
            encrypted: boolean(
                value,
                &[
                    "/StorageEncrypted",
                    "/AtRestEncryptionEnabled",
                    "/Encrypted",
                    "/properties/encryption/enabled",
                ],
            ),
        },
    ))
}
pub fn operation(
    id: &str,
    result: Result<usize, &super::transport::Error>,
    pages: usize,
    required: bool,
) -> Operation {
    let (coverage, records) = match result {
        Ok(records) => (Coverage::Complete, records),
        Err(e) => (e.coverage(), 0),
    };
    Operation {
        id: id.into(),
        coverage,
        observed_at: Utc::now(),
        records,
        pages,
        attempts: 1,
        required,
    }
}

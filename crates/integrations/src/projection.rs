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
        value
            .pointer(p)
            .and_then(|v| {
                v.as_f64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .filter(|value| value.is_finite())
    })
}
pub fn boolean(value: &Value, paths: &[&str]) -> Option<bool> {
    paths.iter().find_map(|p| {
        value.pointer(p).and_then(|v| {
            v.as_bool()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
    })
}
pub fn timestamp(value: &Value, paths: &[&str]) -> Option<DateTime<Utc>> {
    paths.iter().find_map(|path| {
        let value = value.pointer(path)?;
        if let Some(text) = value.as_str() {
            return DateTime::parse_from_rfc3339(text)
                .ok()
                .map(|time| time.to_utc());
        }
        let seconds = value.as_f64()?;
        if !seconds.is_finite() {
            return None;
        }
        DateTime::from_timestamp(
            seconds.floor() as i64,
            ((seconds - seconds.floor()) * 1_000_000_000.0) as u32,
        )
    })
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
            "failed"
            | "failure"
            | "error"
            | "unhealthy"
            | "impaired"
            | "degraded"
            | "unavailable"
            | "alarm"
            | "storage-full"
            | "incompatible-parameters"
            | "incompatible-network"
            | "incompatible-restore"
            | "inaccessible-encryption-credentials"
            | "inaccessible-encryption-credentials-recoverable"
            | "restore-error",
        ) => ServiceState::Failed,
        Some(
            "pending" | "creating" | "updating" | "starting" | "provisioning" | "queued"
            | "working" | "in_progress",
        ) => ServiceState::Starting,
        _ => ServiceState::Unknown,
    }
}
pub fn observation(job: &Job, operation: &str, name: &str, data: Data) -> Observation {
    let resource = format!("{}/{}/{}", job.target.name, operation, identity(name));
    let expected = job
        .target
        .expectations
        .iter()
        .filter(|(selector, _)| resource.contains(selector.as_str()))
        .max_by_key(|(selector, _)| selector.len())
        .map_or(job.target.expected, |(_, expected)| *expected);
    Observation {
        resource,
        operation: operation.into(),
        observed_at: Utc::now(),
        expected,
        data,
    }
}
pub fn service(job: &Job, operation: &str, value: &Value) -> Option<Observation> {
    let name = if job.target.provider == Provider::Azure {
        text(value, &["/id"])
    } else {
        None
    }
    .or_else(|| {
        text(
            value,
            &[
                "/name",
                "/id",
                "/arn",
                "/Arn",
                "/Name",
                "/DBInstanceIdentifier",
                "/DBClusterIdentifier",
                "/ReplicationGroupId",
                "/CacheClusterId",
                "/TableName",
                "/FunctionName",
                "/RepositoryName",
                "/QueueUrl",
                "/InstanceId",
                "/AutoScalingGroupName",
                "/VolumeId",
                "/volumeId",
                "/instanceId",
                "/KeyId",
                "/LoadBalancerArn",
            ],
        )
    })?;
    if !job.target.resources.is_empty() && !job.target.resources.iter().any(|s| name.contains(s)) {
        return None;
    }
    let state = state(text(
        value,
        &[
            "/state",
            "/status",
            "/State/Name",
            "/instanceState/name",
            "/State/Code",
            "/State",
            "/Status",
            "/DBInstanceStatus",
            "/DBClusterStatus",
            "/CacheClusterStatus",
            "/TableStatus",
            "/properties/powerState/code",
            "/properties/state",
            "/properties/status",
            "/properties/provisioningState",
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
                    "/encrypted",
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

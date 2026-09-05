//! AWS certificates, bucket controls, and recovery metadata without payload retention.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::projection::{boolean, number, observation, state, text, timestamp};
use serde_json::Value;
pub fn project(job: &Job, endpoint: &Endpoint, value: &Value) -> Option<Vec<Observation>> {
    let family = endpoint.id.split('/').next()?;
    let id = &endpoint.id;
    let name = text(
        value,
        &[
            "/ResourceArn",
            "/CertificateArn",
            "/FunctionName",
            "/RecoveryPointArn",
            "/volumeId",
            "/VolumeId",
            "/instanceId",
            "/InstanceId",
            "/Id",
            "/AutoScalingGroupName",
            "/serviceArn",
            "/serviceName",
        ],
    )
    .unwrap_or("configuration");
    let data = match family {
        "autoscaling" => Data::Workload {
            desired: number(value, &["/DesiredCapacity"]).unwrap_or(0.0) as u32,
            ready: crate::projection_rows::rows(value, "/Instances/member")
                .into_iter()
                .filter(|instance| {
                    text(instance, &["/HealthStatus"]) == Some("Healthy")
                        && text(instance, &["/LifecycleState"]) == Some("InService")
                })
                .count() as u32,
            created_at: timestamp(value, &["/CreatedTime"]),
            draining: false,
            node: false,
        },
        "ecs-services-detail" => Data::Workload {
            desired: number(value, &["/desiredCount"]).unwrap_or(0.0) as u32,
            ready: number(value, &["/runningCount"]).unwrap_or(0.0) as u32,
            created_at: timestamp(value, &["/createdAt"]),
            draining: text(value, &["/status"]) == Some("DRAINING"),
            node: false,
        },
        "cloudfront" => Data::Service {
            state: match (boolean(value, &["/Enabled"]), text(value, &["/Status"])) {
                (Some(false), _) => ServiceState::Stopped,
                (Some(true), Some("Deployed")) => ServiceState::Ready,
                (Some(true), Some("InProgress")) => ServiceState::Starting,
                _ => ServiceState::Unknown,
            },
            replicas: None,
            backup_enabled: None,
            encrypted: None,
        },
        "ebs" => Data::Service {
            state: match text(value, &["/status", "/State"]) {
                Some("in-use" | "available") => ServiceState::Ready,
                Some("error") => ServiceState::Failed,
                Some("creating" | "deleting") => ServiceState::Starting,
                _ => ServiceState::Unknown,
            },
            replicas: None,
            backup_enabled: None,
            encrypted: boolean(value, &["/encrypted", "/Encrypted"]),
        },
        "dynamodb-backups" => Data::Recovery {
            state: ServiceState::Unknown,
            enabled: text(
                value,
                &["/PointInTimeRecoveryDescription/PointInTimeRecoveryStatus"],
            )
            .map(|status| status == "ENABLED"),
            last_attempt: None,
            last_success: timestamp(
                value,
                &["/PointInTimeRecoveryDescription/LatestRestorableDateTime"],
            ),
            retention_days: None,
            point_in_time: text(
                value,
                &["/PointInTimeRecoveryDescription/PointInTimeRecoveryStatus"],
            )
            .map(|status| status == "ENABLED"),
            geo_redundant: None,
        },
        "acm-detail" => Data::Certificate {
            issued: text(value, &["/Status"]).map(|status| status == "ISSUED"),
            expires_at: timestamp(value, &["/NotAfter"]),
        },
        "s3-encryption" => {
            let rules = value.get("Rule").map(|rules| {
                rules
                    .as_array()
                    .map(|values| values.iter().collect::<Vec<_>>())
                    .unwrap_or_else(|| vec![rules])
            });
            Data::Condition {
                rule: "bucket-encryption-unavailable".into(),
                healthy: rules.map(|rules| {
                    !rules.is_empty()
                        && rules.iter().all(|rule| {
                            matches!(
                                text(rule, &["/ApplyServerSideEncryptionByDefault/SSEAlgorithm"]),
                                Some("AES256" | "aws:kms" | "aws:kms:dsse")
                            )
                        })
                }),
            }
        }
        "s3-versioning" => Data::Inventory {
            family: format!(
                "bucket-versioning/{}",
                text(value, &["/Status"])
                    .filter(|status| matches!(*status, "Enabled" | "Suspended"))
                    .unwrap_or("unset")
            ),
            supported: true,
        },
        "s3-replication" => Data::Inventory {
            family: "bucket-replication-configured".into(),
            supported: true,
        },
        "s3-publicAccessBlock" => {
            let blocked = [
                "/BlockPublicAcls",
                "/IgnorePublicAcls",
                "/BlockPublicPolicy",
                "/RestrictPublicBuckets",
            ]
            .iter()
            .all(|path| boolean(value, &[path]) == Some(true));
            Data::Inventory {
                family: format!(
                    "bucket-public-access/{}",
                    if blocked {
                        "blocked"
                    } else {
                        "partial-or-unknown"
                    }
                ),
                supported: true,
            }
        }
        "recovery-points" => {
            let state = match text(value, &["/Status"]) {
                Some("COMPLETED") => ServiceState::Ready,
                Some("PARTIAL" | "EXPIRED") => ServiceState::Failed,
                other => state(other),
            };
            let at = timestamp(value, &["/CompletionDate", "/CreationDate"]);
            Data::Recovery {
                last_attempt: at,
                state,
                enabled: None,
                last_success: if state == ServiceState::Ready {
                    at
                } else {
                    None
                },
                retention_days: number(value, &["/Lifecycle/DeleteAfterDays"])
                    .map(|days| days as u32),
                point_in_time: None,
                geo_redundant: None,
            }
        }
        "lambda-detail" | "lambda-image" => Data::Service {
            state: state(text(value, &["/State", "/Configuration/State"])),
            replicas: None,
            backup_enabled: None,
            encrypted: None,
        },
        _ => return None,
    };
    Some(vec![observation(job, id, name, data)])
}

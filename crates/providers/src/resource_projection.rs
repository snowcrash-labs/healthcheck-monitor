//! Service-specific health semantics before evidence enters the policy engine.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::projection::{self, boolean, number, observation, text, timestamp};
use serde_json::Value;

pub fn rows<'a>(payload: &'a Value, path: &str) -> Vec<&'a Value> {
    match payload.pointer(path) {
        Some(Value::Array(values)) => values.iter().collect(),
        Some(Value::Object(values)) if path == "/items" => values
            .values()
            .flat_map(|v| {
                v.as_object()
                    .into_iter()
                    .flat_map(|m| m.values())
                    .filter_map(Value::as_array)
                    .flatten()
            })
            .collect(),
        Some(value) if value.is_object() => vec![value],
        _ => vec![],
    }
}
pub fn project(job: &Job, endpoint: &Endpoint, value: &Value) -> Vec<Observation> {
    let id = endpoint.id.as_str();
    let family = id.split('/').next().unwrap_or(id);
    let name = text(
        value,
        &[
            "/name",
            "/id",
            "/arn",
            "/Arn",
            "/Name",
            "/DBInstanceIdentifier",
            "/DBClusterIdentifier",
            "/CacheClusterId",
            "/ReplicationGroupId",
            "/TableName",
            "/FunctionName",
            "/repositoryName",
            "/RepositoryName",
            "/QueueUrl",
            "/instanceId",
            "/InstanceId",
            "/volumeId",
            "/AutoScalingGroupName",
            "/VolumeId",
            "/LoadBalancerArn",
            "/TargetGroupArn",
            "/KeyId",
            "/logGroupName",
            "/TopicArn",
            "/projectId",
            "/subscriptionId",
        ],
    )
    .or_else(|| value.as_str())
    .unwrap_or("resource");
    if !job.target.resources.is_empty()
        && !job
            .target
            .resources
            .iter()
            .any(|selector| name.contains(selector))
    {
        return vec![];
    }
    let obs = |data| observation(job, id, name, data);
    if let Some(instances) = value
        .pointer("/instancesSet/item")
        .and_then(Value::as_array)
    {
        return instances
            .iter()
            .flat_map(|v| project(job, endpoint, v))
            .collect();
    }
    if matches!(family, "builds" | "build-details") {
        let pipeline = text(value, &["/buildTriggerId", "/projectName"]).unwrap_or("");
        let revision = text(
            value,
            &[
                "/sourceProvenance/resolvedRepoSource/commitSha",
                "/substitutions/COMMIT_SHA",
                "/substitutions/SHORT_SHA",
                "/resolvedSourceVersion",
            ],
        )
        .unwrap_or("");
        return vec![obs(Data::Build {
            superseded: false,
            pipeline: projection::identity(pipeline),
            revision: projection::identity(revision),
            target: text(
                value,
                &[
                    "/substitutions/_APP_NAME",
                    "/substitutions/_SERVICE_NAME",
                    "/projectName",
                ],
            )
            .map(projection::identity)
            .unwrap_or_default(),
            state: projection::state(text(value, &["/status", "/buildStatus"])),
            created_at: timestamp(value, &["/createTime", "/startTime"])
                .unwrap_or_else(chrono::Utc::now),
        })];
    }
    if matches!(
        family,
        "open-alerts" | "alerts" | "health" | "provider-health" | "service-health"
    ) {
        let closed = matches!(
            text(
                value,
                &[
                    "/state",
                    "/statusCode",
                    "/properties/status",
                    "/properties/essentials/monitorCondition"
                ]
            ),
            Some("CLOSED" | "closed" | "Resolved" | "resolved")
        );
        return vec![obs(Data::Condition {
            rule: format!("{family}-active"),
            healthy: Some(closed),
        })];
    }
    if family == "sqs-attributes" {
        return [
            "ApproximateNumberOfMessages",
            "ApproximateNumberOfMessagesNotVisible",
            "ApproximateNumberOfMessagesDelayed",
        ]
        .into_iter()
        .filter_map(|key| {
            number(value, &[&format!("/Attributes/{key}")]).map(|value| {
                observation(
                    job,
                    id,
                    key,
                    Data::Metric {
                        name: key.into(),
                        value,
                        capacity: None,
                        warning: None,
                        error: None,
                        window_seconds: 0,
                    },
                )
            })
        })
        .collect();
    }
    if family == "backend-health" || family == "target-health" {
        let health = text(value, &["/healthState", "/TargetHealth/State"]);
        return vec![obs(Data::Condition {
            rule: "load-balancer-backend-unhealthy".into(),
            healthy: health.map(|v| matches!(v, "HEALTHY" | "healthy" | "unused")),
        })];
    }
    if family == "instance-status" {
        let status = text(value, &["/instanceStatus/status"]);
        return vec![obs(Data::Condition {
            rule: "instance-status-impaired".into(),
            healthy: status.map(|v| v == "ok"),
        })];
    }
    if matches!(family, "certificates" | "acm-detail") {
        return vec![obs(Data::Condition {
            rule: "certificate-not-issued".into(),
            healthy: text(value, &["/Status", "/properties/provisioningState"])
                .map(|v| matches!(v, "ISSUED" | "Succeeded")),
        })];
    }
    if family == "cloud-run" {
        let conditions = value
            .pointer("/terminalCondition")
            .or_else(|| value.pointer("/status/conditions"));
        let state = conditions
            .and_then(|v| text(v, &["/state"]))
            .map(|s| {
                if s == "CONDITION_SUCCEEDED" {
                    ServiceState::Ready
                } else if s == "CONDITION_FAILED" {
                    ServiceState::Failed
                } else {
                    ServiceState::Starting
                }
            })
            .unwrap_or(ServiceState::Unknown);
        return vec![obs(Data::Service {
            state,
            replicas: None,
            backup_enabled: None,
            encrypted: None,
        })];
    }
    if family == "autoscaling" {
        let desired = number(value, &["/DesiredCapacity"]).unwrap_or(0.0) as u32;
        let ready = value
            .pointer("/Instances/member")
            .and_then(Value::as_array)
            .map_or(0, |a| {
                a.iter()
                    .filter(|v| text(v, &["/HealthStatus"]) == Some("Healthy"))
                    .count() as u32
            });
        return vec![obs(Data::Workload {
            desired,
            ready,
            created_at: None,
            draining: false,
            node: false,
        })];
    }
    if family == "ecs-services-detail" {
        return vec![obs(Data::Workload {
            desired: number(value, &["/desiredCount"]).unwrap_or(0.0) as u32,
            ready: number(value, &["/runningCount"]).unwrap_or(0.0) as u32,
            created_at: timestamp(value, &["/createdAt"]),
            draining: text(value, &["/status"]) == Some("DRAINING"),
            node: false,
        })];
    }
    let mut result = projection::service(job, id, value)
        .into_iter()
        .collect::<Vec<_>>();
    if result.is_empty() {
        result.push(obs(Data::Inventory {
            family: family.into(),
            supported: false,
        }));
    }
    if let Some(Observation {
        data: Data::Service {
            state, encrypted, ..
        },
        ..
    }) = result.first_mut()
    {
        if *state == ServiceState::Unknown {
            *state = projection::state(text(
                value,
                &[
                    "/instanceState/name",
                    "/State/Code",
                    "/cluster/status",
                    "/State",
                    "/buildStatus",
                    "/provisioningState",
                    "/properties/powerState/code",
                ],
            ));
        }
        if encrypted.is_none() {
            *encrypted = boolean(value, &["/encrypted"]);
        }
    }
    result
}

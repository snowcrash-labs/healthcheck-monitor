//! Project resource health without environment values, annotations, or raw events.
use super::projection::{boolean, number, observation, text, timestamp};
use monitor_core::{config::resolve::Job, model::*};
use serde_json::Value;

fn condition(value: &Value, kind: &str) -> Option<bool> {
    value
        .pointer("/status/conditions")
        .and_then(Value::as_array)?
        .iter()
        .find(|v| text(v, &["/type"]) == Some(kind))
        .and_then(|v| text(v, &["/status"]))
        .and_then(|v| match v {
            "True" => Some(true),
            "False" => Some(false),
            _ => None,
        })
}
pub fn project(job: &Job, kind: &str, value: &Value) -> Vec<Observation> {
    let Some(name) = text(value, &["/metadata/name"]) else {
        return vec![];
    };
    if !job.target.resources.is_empty()
        && !job
            .target
            .resources
            .iter()
            .any(|selector| name.contains(selector))
    {
        return vec![];
    }
    let namespace = text(value, &["/metadata/namespace"]).unwrap_or("_cluster");
    let name = format!("{namespace}/{name}");
    let created_at = timestamp(value, &["/metadata/creationTimestamp"]);
    let mut output = Vec::new();
    super::kube_links::project(job, kind, &name, value, &mut output);
    let num = |path| number(value, &[path]).unwrap_or(0.0) as u32;
    let data = match kind {
        "jobs" => Data::Job {
            complete: condition(value, "Complete") == Some(true),
            failed: condition(value, "Failed") == Some(true),
            failed_attempts: num("/status/failed"),
            succeeded: num("/status/succeeded"),
            active: num("/status/active"),
            created_at,
        },
        "cronjobs" => Data::Schedule {
            schedule: text(value, &["/spec/schedule"]).unwrap_or("").into(),
            timezone: text(value, &["/spec/timeZone"]).unwrap_or("UTC").into(),
            suspended: boolean(value, &["/spec/suspend"]).unwrap_or(false),
            active: value
                .pointer("/status/active")
                .and_then(Value::as_array)
                .map_or(0, |a| a.len() as u32),
            last_schedule: timestamp(value, &["/status/lastScheduleTime"]),
            last_success: timestamp(value, &["/status/lastSuccessfulTime"]),
        },
        "deployments" | "statefulsets" | "daemonsets" | "nodes" => {
            let created_at = value
                .pointer("/status/conditions")
                .and_then(Value::as_array)
                .and_then(|conditions| {
                    conditions.iter().find(|condition| {
                        text(condition, &["/type"]) == Some("Progressing")
                            && matches!(
                                text(condition, &["/reason"]),
                                Some("NewReplicaSetCreated" | "ReplicaSetUpdated")
                            )
                    })
                })
                .and_then(|condition| {
                    timestamp(condition, &["/lastUpdateTime", "/lastTransitionTime"])
                })
                .or(created_at);
            let node = kind == "nodes";
            let desired = if node {
                1
            } else if kind == "daemonsets" {
                num("/status/desiredNumberScheduled")
            } else {
                number(value, &["/spec/replicas"]).unwrap_or(1.0) as u32
            };
            let ready = if node {
                u32::from(condition(value, "Ready") == Some(true))
            } else {
                number(value, &["/status/readyReplicas", "/status/numberReady"]).unwrap_or(0.0)
                    as u32
            };
            let draining = value.pointer("/metadata/deletionTimestamp").is_some()
                || value
                    .pointer("/spec/taints")
                    .and_then(Value::as_array)
                    .is_some_and(|taints| {
                        taints.iter().any(|t| {
                            matches!(
                                text(t, &["/key"]),
                                Some(
                                    "ToBeDeletedByClusterAutoscaler"
                                        | "DeletionCandidateOfClusterAutoscaler"
                                )
                            )
                        })
                    });
            Data::Workload {
                desired,
                ready,
                created_at,
                draining,
                node,
            }
        }
        "pods" => {
            if text(value, &["/status/phase"]) == Some("Failed")
                && created_at.is_some_and(|at| {
                    (chrono::Utc::now() - at).num_seconds() > job.settings.runtime_window.0 as i64
                })
            {
                return vec![];
            }
            if text(value, &["/status/phase"]) == Some("Succeeded") {
                return output;
            }
            if let Some(containers) = value
                .pointer("/status/containerStatuses")
                .and_then(Value::as_array)
            {
                for container in containers.iter().take(128) {
                    let container_name = text(container, &["/name"]).unwrap_or("unknown");
                    output.push(observation(
                        job,
                        kind,
                        &format!("{name}/{container_name}"),
                        Data::Pod {
                            uid: text(value, &["/metadata/uid"]).unwrap_or("").into(),
                            container: container_name.into(),
                            ready: boolean(container, &["/ready"]).unwrap_or(false),
                            restarts: number(container, &["/restartCount"]).unwrap_or(0.0) as u32,
                            crash_loop: matches!(
                                text(container, &["/state/waiting/reason"]),
                                Some("CrashLoopBackOff" | "ImagePullBackOff")
                            ),
                            created_at,
                            terminated_at: timestamp(
                                container,
                                &["/lastState/terminated/finishedAt"],
                            ),
                        },
                    ));
                }
                images(job, kind, &name, value, &mut output);
                return output;
            }
            Data::Workload {
                desired: 1,
                ready: 0,
                created_at,
                draining: false,
                node: false,
            }
        }
        "certificates" | "externalsecrets" | "scaledobjects" => Data::Condition {
            rule: format!("{kind}-not-ready"),
            healthy: condition(value, "Ready"),
        },
        "horizontalpodautoscalers" => Data::Condition {
            rule: "autoscaler-unavailable".into(),
            healthy: condition(value, "AbleToScale"),
        },
        "events" => {
            if text(value, &["/type"]) != Some("Warning") {
                return output;
            }
            let last = timestamp(
                value,
                &[
                    "/lastTimestamp",
                    "/eventTime",
                    "/metadata/creationTimestamp",
                ],
            );
            let Some(last_seen) = last.filter(|at| {
                (chrono::Utc::now() - *at).num_seconds() <= job.settings.log_window.0 as i64
            }) else {
                return vec![];
            };
            Data::Log {
                signature: LogClass::Warning,
                count: number(value, &["/count"]).unwrap_or(1.0) as u64,
                first_seen: timestamp(value, &["/firstTimestamp"]).unwrap_or(last_seen),
                last_seen,
                sampled: true,
            }
        }
        _ => Data::Inventory {
            family: kind.into(),
            supported: true,
        },
    };
    output.push(observation(job, kind, &name, data));
    images(job, kind, &name, value, &mut output);
    output
}
fn images(job: &Job, kind: &str, name: &str, value: &Value, output: &mut Vec<Observation>) {
    let containers = [
        "/spec/containers",
        "/spec/template/spec/containers",
        "/spec/jobTemplate/spec/template/spec/containers",
    ]
    .into_iter()
    .find_map(|p| value.pointer(p).and_then(Value::as_array));
    if let Some(containers) = containers {
        for container in containers.iter().take(128) {
            let Some(image) = text(container, &["/image"]) else {
                continue;
            };
            let container_name = text(container, &["/name"]).unwrap_or("unknown");
            let digest = value
                .pointer("/status/containerStatuses")
                .and_then(Value::as_array)
                .and_then(|items| {
                    items
                        .iter()
                        .find(|s| text(s, &["/name"]) == Some(container_name))
                })
                .and_then(|v| text(v, &["/imageID"]));
            let (desired, observed_digest, revision) = image_parts(image, digest);
            output.push(observation(
                job,
                kind,
                &format!("{name}/image/{container_name}"),
                Data::Image {
                    desired,
                    observed_digest,
                    revision,
                },
            ));
        }
    }
}
pub fn image_parts(
    image: &str,
    observed: Option<&str>,
) -> (String, Option<String>, Option<String>) {
    let digest = observed
        .and_then(|s| s.split_once('@').map(|(_, d)| d))
        .or_else(|| image.split_once('@').map(|(_, d)| d))
        .map(super::projection::identity);
    let tag = image
        .rsplit('/')
        .next()
        .and_then(|s| s.split_once(':').map(|(_, t)| t))
        .unwrap_or("");
    let revision = tag
        .strip_prefix("release-")
        .or_else(|| tag.strip_prefix("sha-"))
        .filter(|s| (7..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(String::from);
    (super::projection::identity(image), digest, revision)
}

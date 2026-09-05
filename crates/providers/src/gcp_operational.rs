//! Compute quota usage and Cloud SQL recovery history projected into shared policies.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::projection::{number, observation, state, text, timestamp};
use serde_json::Value;
pub fn project(job: &Job, endpoint: &Endpoint, value: &Value) -> Option<Vec<Observation>> {
    let family = endpoint.id.split('/').next()?;
    if family == "cloud-run" {
        let name = text(value, &["/name"]).unwrap_or("service");
        let state = match text(value, &["/terminalCondition/state"]) {
            Some("CONDITION_SUCCEEDED") => ServiceState::Ready,
            Some("CONDITION_FAILED") => ServiceState::Failed,
            Some("CONDITION_PENDING" | "CONDITION_RECONCILING") => ServiceState::Starting,
            _ => ServiceState::Unknown,
        };
        return Some(vec![observation(
            job,
            &endpoint.id,
            name,
            Data::Service {
                state,
                replicas: None,
                backup_enabled: None,
                encrypted: None,
            },
        )]);
    }
    if family == "instance-group-managers" {
        let name = text(value, &["/name"]).unwrap_or("group");
        let stable = value.pointer("/status/isStable").and_then(Value::as_bool);
        return Some(vec![observation(
            job,
            &endpoint.id,
            name,
            Data::Service {
                state: match stable {
                    Some(true) => ServiceState::Ready,
                    Some(false) => ServiceState::Starting,
                    None => ServiceState::Unknown,
                },
                replicas: number(value, &["/targetSize"]).map(|size| size as u32),
                backup_enabled: None,
                encrypted: None,
            },
        )]);
    }
    if family == "scheduler" {
        let name = text(value, &["/name"]).unwrap_or("schedule");
        let attempt = timestamp(value, &["/lastAttemptTime"]);
        return Some(vec![observation(
            job,
            &endpoint.id,
            name,
            Data::Schedule {
                created_at: timestamp(value, &["/userUpdateTime"]),
                starting_deadline_seconds: None,
                forbid_overlap: false,
                schedule: text(value, &["/schedule"]).unwrap_or("").into(),
                timezone: text(value, &["/timeZone"]).unwrap_or("UTC").into(),
                suspended: text(value, &["/state"]) == Some("PAUSED"),
                active: 0,
                last_schedule: attempt,
                last_success: if number(value, &["/status/code"]).unwrap_or(0.0) == 0.0 {
                    attempt
                } else {
                    None
                },
            },
        )]);
    }
    if family == "quotas" {
        if let Some(name) = text(value, &["/metric"]) {
            return Some(
                number(value, &["/usage"])
                    .map(|usage| {
                        observation(
                            job,
                            &endpoint.id,
                            name,
                            Data::Metric {
                                name: name.into(),
                                value: usage,
                                capacity: number(value, &["/limit"]).filter(|limit| *limit > 0.0),
                                warning: None,
                                error: None,
                                window_seconds: 0,
                            },
                        )
                    })
                    .into_iter()
                    .collect(),
            );
        }
        return Some(
            value
                .get("quotas")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .take(job.settings.max_assets + 1)
                .filter_map(|quota| {
                    let name = text(quota, &["/metric"])?;
                    let usage = number(quota, &["/usage"])?;
                    Some(observation(
                        job,
                        &endpoint.id,
                        name,
                        Data::Metric {
                            name: name.into(),
                            value: usage,
                            capacity: number(quota, &["/limit"]).filter(|limit| *limit > 0.0),
                            warning: None,
                            error: None,
                            window_seconds: 0,
                        },
                    ))
                })
                .collect(),
        );
    }
    if family == "sql-backups" {
        let state = state(text(value, &["/status"]));
        let at = timestamp(value, &["/endTime", "/startTime", "/enqueuedTime"]);
        return Some(vec![observation(
            job,
            &endpoint.id,
            "recovery",
            Data::Recovery {
                state,
                enabled: None,
                last_attempt: at,
                last_success: if state == ServiceState::Ready {
                    at
                } else {
                    None
                },
                retention_days: None,
                point_in_time: None,
                geo_redundant: None,
            },
        )]);
    }
    None
}

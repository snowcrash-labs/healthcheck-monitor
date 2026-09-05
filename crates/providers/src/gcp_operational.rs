//! Compute quota usage and Cloud SQL recovery history projected into shared policies.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::projection::{number, observation, state, text, timestamp};
use serde_json::Value;
pub fn project(job: &Job, endpoint: &Endpoint, value: &Value) -> Option<Vec<Observation>> {
    let family = endpoint.id.split('/').next()?;
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

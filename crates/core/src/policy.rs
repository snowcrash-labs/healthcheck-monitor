//! Health policy over allowlisted observations; collection status is independent.
use crate::{config::settings::Settings, model::*};
use chrono::{DateTime, Utc};

#[derive(Debug)]
pub struct Evaluation {
    pub health: Health,
    pub findings: Vec<Finding>,
}
fn result(health: Health) -> Evaluation {
    Evaluation {
        health,
        findings: vec![],
    }
}
fn fault(obs: &Observation, rule: &str, severity: Severity, confidence: Confidence) -> Evaluation {
    Evaluation {
        health: if severity == Severity::Error {
            Health::Unhealthy
        } else {
            Health::Degraded
        },
        findings: vec![Finding {
            id: format!("{}/{}", obs.resource, rule),
            resource: obs.resource.clone(),
            rule: rule.into(),
            severity,
            evidence: vec![obs.operation.clone()],
            observed_at: obs.observed_at,
            expected: obs.expected,
            confidence,
            stale: false,
            clear_count: 0,
        }],
    }
}
fn age(at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Option<u64> {
    at.and_then(|t| (now - t).num_seconds().try_into().ok())
}
/// Freshness gates recovery. Job terminal conditions always precede attempt counters.
pub fn evaluate(
    obs: &Observation,
    previous: Option<&Observation>,
    settings: &Settings,
    now: DateTime<Utc>,
) -> Evaluation {
    if age(Some(obs.observed_at), now).is_none_or(|v| v > settings.freshness()) {
        return result(Health::Unknown);
    }
    let inactive = obs.expected != Expected::Active;
    let error = |rule| fault(obs, rule, Severity::Error, Confidence::Direct);
    let warning = |rule| fault(obs, rule, Severity::Warning, Confidence::Direct);
    match &obs.data {
        Data::Endpoint {
            dns,
            tls,
            status,
            accepted,
            latency_ms,
            expires_at,
        } => {
            if !dns
                || !tls
                || !status.is_some_and(|s| {
                    if accepted.is_empty() {
                        (100..500).contains(&s)
                    } else {
                        accepted.contains(&s)
                    }
                })
            {
                return if inactive {
                    result(Health::ExpectedInactive)
                } else {
                    error("endpoint-unreachable")
                };
            }
            if expires_at.is_some_and(|t| (t - now).num_days() < 14) {
                return warning("certificate-expiring");
            }
            if settings
                .latency_error_ms
                .is_some_and(|limit| *latency_ms as f64 > limit)
            {
                return error("endpoint-latency");
            }
        }
        Data::Job {
            complete,
            failed,
            created_at,
            ..
        } => {
            if *complete && *failed {
                return result(Health::Unknown);
            }
            if *complete {
                return result(Health::Healthy);
            }
            if *failed
                && !inactive
                && age(*created_at, now).is_none_or(|v| v <= settings.runtime_window.0)
            {
                return error("job-failed");
            }
            return result(if inactive {
                Health::ExpectedInactive
            } else {
                Health::Unknown
            });
        }
        Data::Workload {
            desired,
            ready,
            created_at,
            draining,
            node,
        } => {
            if *draining || *desired == 0 {
                return result(Health::ExpectedInactive);
            }
            let grace = if *node {
                settings.node_grace.0
            } else {
                settings.rollout_grace.0
            };
            if ready < desired {
                if age(*created_at, now).is_some_and(|v| v < grace) {
                    return result(Health::Unknown);
                }
                if inactive {
                    return result(Health::ExpectedInactive);
                }
                return error(if *node {
                    "node-not-ready"
                } else {
                    "replicas-not-ready"
                });
            }
        }
        Data::Pod {
            uid,
            container,
            ready,
            restarts,
            crash_loop,
            created_at,
            terminated_at,
        } => {
            if inactive {
                return result(Health::ExpectedInactive);
            }
            if *crash_loop {
                return error("container-crash-loop");
            }
            if !ready && age(*created_at, now).is_none_or(|v| v >= settings.rollout_grace.0) {
                return error("pod-not-ready");
            }
            if let Some(Observation {
                data:
                    Data::Pod {
                        uid: old_uid,
                        container: old_container,
                        restarts: old,
                        ..
                    },
                ..
            }) = previous
            {
                if uid == old_uid
                    && container == old_container
                    && restarts.saturating_sub(*old) >= 3
                {
                    return warning("container-restarting");
                }
            } else if *restarts >= 3
                && age(*terminated_at, now).is_some_and(|v| v <= settings.metric_window.0)
            {
                return warning("container-restarting");
            }
            if !ready {
                return result(Health::Unknown);
            }
        }
        Data::Schedule {
            suspended,
            active,
            last_schedule,
            last_success,
            ..
        } => {
            if *suspended || inactive {
                return result(Health::ExpectedInactive);
            }
            if *active > 0 {
                return result(Health::Unknown);
            }
            if last_schedule.is_none() {
                return result(Health::Unknown);
            }
            if last_schedule > last_success
                && age(*last_schedule, now).is_some_and(|v| v > settings.rollout_grace.0)
            {
                return warning("latest-schedule-not-successful");
            }
        }
        Data::Queue {
            backlog,
            activation,
            ready,
            crash_loop,
            scaler_ready,
            age_seconds,
            dead_letters,
            ..
        } => {
            if !backlog.is_finite() || *backlog < 0.0 {
                return result(Health::Unknown);
            }
            if inactive {
                return result(Health::ExpectedInactive);
            }
            if dead_letters.is_some_and(|n| n > 0.0) {
                return warning("dead-letter-backlog");
            }
            if *backlog > *activation {
                if *crash_loop && *ready == 0 {
                    return error("queued-work-crash-looping-consumer");
                }
                if !scaler_ready {
                    return error("queued-work-scaler-not-ready");
                }
                if settings
                    .queue_age_error
                    .zip(*age_seconds)
                    .is_some_and(|(limit, value)| value > limit)
                {
                    return error("queue-age");
                }
                if *ready == 0 {
                    if previous.is_some_and(|p| matches!(&p.data, Data::Queue { backlog: b, ready: 0, .. } if *b > *activation)) {
                        return fault(obs, "persistent-queued-work-consumer-starting", Severity::Warning, Confidence::Correlated);
                    }
                    return result(Health::Unknown);
                }
            }
        }
        Data::Service {
            state,
            backup_enabled,
            encrypted,
            ..
        } => {
            if inactive && *state == ServiceState::Stopped {
                return result(Health::ExpectedInactive);
            }
            match state {
                ServiceState::Failed | ServiceState::Stopped => {
                    return error("service-unavailable");
                }
                ServiceState::Starting | ServiceState::Unknown => return result(Health::Unknown),
                ServiceState::Ready => {}
            }
            if *encrypted == Some(false) {
                return warning("encryption-disabled");
            }
            if *backup_enabled == Some(false) {
                return warning("backup-disabled");
            }
        }
        Data::Metric {
            value,
            capacity,
            warning: warn,
            error: err,
            window_seconds,
            ..
        } => {
            if !value.is_finite() {
                return result(Health::Unknown);
            }
            let (value, warn, err) =
                if let Some(capacity) = capacity.filter(|c| *c > 0.0 && c.is_finite()) {
                    (
                        *value / capacity * 100.0,
                        Some(settings.capacity_warning),
                        Some(settings.capacity_error),
                    )
                } else {
                    (*value, *warn, *err)
                };
            if warn.is_none() && err.is_none() {
                return result(Health::Unknown);
            }
            if *window_seconds < settings.capacity_sustain.0 {
                return result(Health::Unknown);
            }
            if err.is_some_and(|t| value >= t) {
                return error("metric-threshold");
            }
            if warn.is_some_and(|t| value >= t) {
                return warning("metric-threshold");
            }
        }
        Data::Build {
            state: ServiceState::Failed,
            ..
        } => return warning("deployment-blocked"),
        Data::Build {
            state: ServiceState::Ready,
            ..
        } => {}
        Data::Build { .. } => return result(Health::Unknown),
        Data::Image {
            observed_digest,
            revision,
            ..
        } => {
            if observed_digest.is_none() || revision.is_none() {
                return result(Health::Unknown);
            }
        }
        Data::Log {
            signature, count, ..
        } => {
            if *signature != LogClass::Warning && *count > 0 {
                return warning("runtime-failure-sample");
            }
            return result(Health::Unknown);
        }
        Data::Inventory { .. } => return result(Health::Unknown),
        Data::Condition { rule, healthy } => match healthy {
            Some(false) => return error(rule),
            None => return result(Health::Unknown),
            Some(true) => {}
        },
        Data::Identity { .. } => {}
    }
    result(Health::Healthy)
}

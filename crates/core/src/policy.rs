//! Health policy over allowlisted observations; collection status is independent.
use crate::{config::settings::Settings, model::*};
use chrono::{DateTime, Utc};

#[derive(Debug)]
pub struct Evaluation {
    pub health: Health,
    pub findings: Vec<Finding>,
}
pub(crate) fn result(health: Health) -> Evaluation {
    Evaluation {
        health,
        findings: vec![],
    }
}
pub(crate) fn fault(
    obs: &Observation,
    rule: &str,
    severity: Severity,
    confidence: Confidence,
) -> Evaluation {
    Evaluation {
        health: if severity == Severity::Error {
            Health::Unhealthy
        } else {
            Health::Degraded
        },
        findings: vec![Finding {
            check: None,
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
            valid_until: None,
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
    let previous = previous.filter(|prior| {
        prior.resource == obs.resource
            && prior.expected == obs.expected
            && prior.observed_at < obs.observed_at
            && (obs.observed_at - prior.observed_at).num_seconds() <= settings.freshness() as i64
    });
    let inactive = obs.expected != Expected::Active;
    let error = |rule| fault(obs, rule, Severity::Error, Confidence::Direct);
    let warning = |rule| fault(obs, rule, Severity::Warning, Confidence::Direct);
    match &obs.data {
        Data::Provenance { .. } => return crate::releases::evaluate(obs, now),
        Data::Slo { .. } => return crate::slo_policy::evaluate(obs, settings),
        Data::Progress {
            state: Health::Unhealthy,
        } => return fault(obs, "flow-stalled", Severity::Error, Confidence::Correlated),
        Data::Progress { state } => return result(*state),
        Data::Endpoint { .. } => return crate::endpoint_policy::evaluate(obs, settings, now),
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
            ..
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
                && uid == old_uid
                && container == old_container
                && restarts.saturating_sub(*old) >= 3
            {
                return warning("container-restarting");
            }
            if !ready {
                return result(Health::Unknown);
            }
        }
        Data::Schedule { .. } => return crate::schedule_policy::evaluate(obs, settings, now),
        Data::Certificate { issued, expires_at } => {
            if inactive {
                return result(Health::ExpectedInactive);
            }
            if *issued == Some(false) || expires_at.is_some_and(|at| at <= now) {
                return error("certificate-invalid");
            }
            if expires_at
                .is_some_and(|at| (at - now).num_seconds() < settings.certificate_warning.0 as i64)
            {
                return warning("certificate-expiring");
            }
            if issued.is_none() || expires_at.is_none() {
                return result(Health::Unknown);
            }
        }
        Data::Recovery {
            state,
            enabled,
            last_success,
            ..
        } => {
            if inactive {
                return result(Health::ExpectedInactive);
            }
            if *enabled == Some(false) {
                return warning("backup-disabled");
            }
            if *state == ServiceState::Failed {
                return warning("backup-failed");
            }
            if let Some(limit) = settings.recovery_age_error {
                if last_success.is_some_and(|at| (now - at).num_seconds() > limit.0 as i64) {
                    return error("recovery-point-too-old");
                }
                if last_success.is_some() {
                    return result(Health::Healthy);
                }
            }
            return result(Health::Unknown);
        }
        Data::Queue { .. } => return crate::queue_policy::evaluate(obs, previous, settings),
        Data::Service { .. } | Data::Metric { .. } => {
            return crate::resource_policy::evaluate(obs, settings);
        }
        Data::Build {
            state: ServiceState::Failed,
            superseded: false,
            created_at,
            target,
            ..
        } => {
            if created_at.is_none() {
                return result(Health::Unknown);
            }
            if age(*created_at, now).is_some_and(|age| age > settings.runtime_window.0) {
                return result(Health::ExpectedInactive);
            }
            return warning(if target.is_empty() {
                "build-failed"
            } else {
                "deployment-blocked"
            });
        }
        Data::Build {
            state: ServiceState::Ready,
            ..
        } => {}
        Data::Build {
            superseded: true, ..
        } => {}
        Data::Build { .. } => return result(Health::Unknown),
        Data::Image { .. } => return result(Health::Unknown),
        Data::Log {
            signature,
            count,
            sampled,
            ..
        } => {
            if *signature != LogClass::Warning && *count > 0 {
                return warning("runtime-failure-sample");
            }
            if *count == 0 && !sampled {
                return result(Health::Healthy);
            }
            return result(Health::Unknown);
        }
        Data::Registry { .. }
        | Data::Artifact { .. }
        | Data::Commit { .. }
        | Data::SloDefinition { .. }
        | Data::LogWorkspace { .. }
        | Data::LogWindow { .. }
        | Data::MetricResource { .. }
        | Data::Quota { .. }
        | Data::Inventory { .. }
        | Data::Scaler { .. }
        | Data::Owner { .. }
        | Data::AdvertisedEndpoint { .. } => return result(Health::Unknown),
        Data::Condition { rule, healthy } => match healthy {
            Some(false) => return error(rule),
            None => return result(Health::Unknown),
            Some(true) => {}
        },
        Data::Identity { .. } => {}
    }
    result(Health::Healthy)
}

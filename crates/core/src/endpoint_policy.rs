//! Endpoint policy distinguishes configured application health from root reachability.
use crate::{
    config::settings::Settings,
    model::*,
    policy::{Evaluation, fault, result},
};
use chrono::{DateTime, Utc};
pub fn evaluate(obs: &Observation, settings: &Settings, now: DateTime<Utc>) -> Evaluation {
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
            if expires_at
                .is_some_and(|t| (t - now).num_seconds() < settings.certificate_warning.0 as i64)
            {
                return warning("certificate-expiring");
            }
            if settings
                .latency_error_ms
                .is_some_and(|limit| *latency_ms as f64 > limit)
            {
                return error("endpoint-latency");
            }
        }
        _ => return result(Health::Unknown),
    }
    result(Health::Healthy)
}

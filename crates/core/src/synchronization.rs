//! ExternalSecret readiness and periodic refresh freshness are separate conditions.
use crate::{
    config::settings::Settings,
    model::*,
    policy::{Evaluation, fault, result},
};
use chrono::{DateTime, Utc};
pub fn evaluate(obs: &Observation, settings: &Settings, now: DateTime<Utc>) -> Evaluation {
    let Data::Synchronization {
        ready,
        last_sync,
        interval_seconds,
    } = obs.data
    else {
        return result(Health::Unknown);
    };
    if obs.expected != Expected::Active {
        return result(Health::ExpectedInactive);
    }
    if ready == Some(false) {
        return fault(
            obs,
            "externalsecrets-not-ready",
            Severity::Error,
            Confidence::Direct,
        );
    }
    if ready != Some(true) {
        return result(Health::Unknown);
    }
    if let Some(interval) = interval_seconds {
        let Some(last) = last_sync else {
            return result(Health::Unknown);
        };
        let age = (now - last).num_seconds();
        if age < 0 {
            return result(Health::Unknown);
        }
        if age as u64
            > interval
                .saturating_mul(2)
                .saturating_add(settings.operation_timeout.0)
        {
            return fault(
                obs,
                "externalsecret-sync-stale",
                Severity::Warning,
                Confidence::Direct,
            );
        }
    }
    result(Health::Healthy)
}

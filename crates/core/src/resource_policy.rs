//! Managed resource state and sustained metric thresholds.
use crate::{
    config::settings::Settings,
    model::*,
    policy::{Evaluation, fault, result},
};
pub(crate) fn evaluate(obs: &Observation, settings: &Settings) -> Evaluation {
    let inactive = obs.expected != Expected::Active;
    let error = |rule| fault(obs, rule, Severity::Error, Confidence::Direct);
    let warning = |rule| fault(obs, rule, Severity::Warning, Confidence::Direct);
    match &obs.data {
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
                        warn.or(Some(settings.capacity_warning)),
                        err.or(Some(settings.capacity_error)),
                    )
                } else {
                    (*value, *warn, *err)
                };
            if warn.is_none() && err.is_none() {
                return result(Health::Unknown);
            }
            if capacity.is_some() && *window_seconds < settings.capacity_sustain.0 {
                return result(Health::Unknown);
            }
            if err.is_some_and(|t| value >= t) {
                return error("metric-threshold");
            }
            if warn.is_some_and(|t| value >= t) {
                return warning("metric-threshold");
            }
        }
        _ => return result(Health::Unknown),
    }
    result(Health::Healthy)
}

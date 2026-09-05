//! Sustained capacity pressure from distinct current readings, with separate warning/error clocks.
use crate::{
    config::settings::Settings,
    model::*,
    policy::{Evaluation, fault, result},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pressure {
    last_seen: DateTime<Utc>,
    warning_since: Option<DateTime<Utc>>,
    error_since: Option<DateTime<Utc>>,
    warning: f64,
    error: f64,
}
pub fn evaluate(
    state: &mut BTreeMap<String, Pressure>,
    obs: &Observation,
    settings: &Settings,
    complete: bool,
    now: DateTime<Utc>,
) -> Option<Evaluation> {
    let Data::Metric {
        value,
        capacity: Some(capacity),
        warning,
        error,
        window_seconds,
        ..
    } = &obs.data
    else {
        return None;
    };
    if obs.expected != Expected::Active {
        state.remove(&obs.resource);
        return Some(result(Health::ExpectedInactive));
    }
    if !complete
        || obs.observed_at > now
        || (now - obs.observed_at).num_seconds() > settings.freshness() as i64
    {
        state.remove(&obs.resource);
        return None;
    }
    if *window_seconds >= settings.capacity_sustain.0 {
        return None;
    }
    if !value.is_finite() || *value < 0.0 || !capacity.is_finite() || *capacity <= 0.0 {
        state.remove(&obs.resource);
        return Some(result(Health::Unknown));
    }
    let value = value / capacity * 100.0;
    let warning = warning.unwrap_or(settings.capacity_warning);
    let error = error.unwrap_or(settings.capacity_error);
    if state.len() >= settings.max_assets && !state.contains_key(&obs.resource) {
        return Some(result(Health::Unknown));
    }
    let pressure = state.entry(obs.resource.clone()).or_insert(Pressure {
        last_seen: obs.observed_at,
        warning_since: None,
        error_since: None,
        warning,
        error,
    });
    if obs.observed_at < pressure.last_seen {
        return Some(result(Health::Unknown));
    }
    if (obs.observed_at - pressure.last_seen).num_seconds() > settings.freshness() as i64
        || pressure.warning != warning
        || pressure.error != error
    {
        pressure.warning_since = None;
        pressure.error_since = None;
    }
    pressure.warning = warning;
    pressure.error = error;
    pressure.last_seen = obs.observed_at;
    if value >= warning {
        pressure.warning_since.get_or_insert(obs.observed_at);
    } else {
        pressure.warning_since = None;
    }
    if value >= error {
        pressure.error_since.get_or_insert(obs.observed_at);
    } else {
        pressure.error_since = None;
    }
    let sustained = |since: Option<DateTime<Utc>>| {
        since.is_some_and(|at| {
            (obs.observed_at - at).num_seconds() >= settings.capacity_sustain.0 as i64
        })
    };
    if sustained(pressure.error_since) {
        return Some(fault(
            obs,
            "metric-threshold",
            Severity::Error,
            Confidence::Correlated,
        ));
    }
    if sustained(pressure.warning_since) {
        return Some(fault(
            obs,
            "metric-threshold",
            Severity::Warning,
            Confidence::Correlated,
        ));
    }
    Some(result(if value < warning {
        Health::Healthy
    } else {
        Health::Unknown
    }))
}

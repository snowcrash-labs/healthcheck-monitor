//! Compliance is a long-term objective; urgent burn thresholds require explicit configuration.
use crate::{
    config::settings::Settings,
    model::*,
    policy::{Evaluation, fault, result},
};
pub fn evaluate(obs: &Observation, settings: &Settings) -> Evaluation {
    let Data::Slo {
        goal,
        compliance,
        budget,
        burn_rate,
        ..
    } = &obs.data
    else {
        return result(Health::Unknown);
    };
    if obs.expected != Expected::Active {
        return result(Health::ExpectedInactive);
    }
    if !goal.is_finite()
        || *goal <= 0.0
        || *goal > 1.0
        || compliance.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
    {
        return result(Health::Unknown);
    }
    if settings
        .slo_burn_rate_error
        .zip(*burn_rate)
        .is_some_and(|(threshold, value)| value.is_finite() && value >= threshold)
    {
        return fault(obs, "slo-burn-rate", Severity::Error, Confidence::Direct);
    }
    if compliance.is_some_and(|value| value < *goal)
        || budget.is_some_and(|value| value.is_finite() && value < 0.0)
    {
        return fault(obs, "slo-compliance", Severity::Warning, Confidence::Direct);
    }
    result(if compliance.is_some() {
        Health::Healthy
    } else {
        Health::Unknown
    })
}

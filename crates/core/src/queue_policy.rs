//! Queue demand and consumer health correlation.
use crate::{
    config::settings::Settings,
    model::*,
    policy::{Evaluation, fault, result},
};
pub(crate) fn evaluate(
    obs: &Observation,
    previous: Option<&Observation>,
    settings: &Settings,
) -> Evaluation {
    let inactive = obs.expected != Expected::Active;
    let error = |rule| fault(obs, rule, Severity::Error, Confidence::Direct);
    let warning = |rule| fault(obs, rule, Severity::Warning, Confidence::Direct);
    match &obs.data {
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
        _ => return result(Health::Unknown),
    }
    result(Health::Healthy)
}

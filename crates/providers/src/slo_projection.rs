//! Normalize provider-configured SLO definitions before querying their telemetry.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::projection::{number, observation, text};
use serde_json::Value;
pub fn gcp(job: &Job, endpoint: &Endpoint, value: &Value) -> Vec<Observation> {
    let Some(name) =
        text(value, &["/name"]).filter(|name| monitor_core::config::validate::identifier(name))
    else {
        return vec![];
    };
    let Some((_, suffix)) = name.split_once("/services/") else {
        return vec![];
    };
    let name = format!("projects/{}/services/{suffix}", job.target.scope);
    if !job.target.resources.is_empty()
        && !job
            .target
            .resources
            .iter()
            .any(|selector| name.contains(selector))
    {
        return vec![];
    }
    let goal = number(value, &["/goal"]).filter(|goal| *goal > 0.0 && *goal <= 1.0);
    let period_seconds = text(value, &["/rollingPeriod"])
        .and_then(|period| period.strip_suffix('s'))
        .and_then(|period| period.parse::<u64>().ok());
    vec![observation(
        job,
        &endpoint.id,
        &name,
        Data::SloDefinition {
            name: name.clone(),
            goal,
            period_seconds,
        },
    )]
}
pub fn configured(job: &Job, result: &mut CheckResult) {
    let mut found = std::collections::BTreeSet::new();
    for obs in &mut result.observations {
        if let Data::Metric {
            name,
            value,
            capacity,
            ..
        } = &obs.data
            && let Some(goal) = job.target.slo_goals.get(name)
        {
            let compliance = *value / capacity.unwrap_or(1.0);
            let valid = compliance.is_finite() && (0.0..=1.0).contains(&compliance);
            if !valid {
                for operation in &mut result.operations {
                    if operation.id == obs.operation {
                        operation.coverage = Coverage::Malformed;
                    }
                }
            }
            found.insert(name.clone());
            obs.resource.push_str("/slo");
            obs.data = Data::Slo {
                goal: *goal,
                compliance: valid.then_some(compliance),
                budget: None,
                burn_rate: None,
                period_seconds: None,
            };
        }
    }
    for name in job
        .target
        .slo_goals
        .keys()
        .filter(|name| !found.contains(*name))
    {
        result
            .operations
            .push(monitor_integrations::projection::operation(
                name,
                Err(&monitor_integrations::transport::Error::Missing),
                0,
                true,
            ));
    }
}

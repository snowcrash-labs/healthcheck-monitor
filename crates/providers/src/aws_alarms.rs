//! Native CloudWatch metric, composite, and log alarms without action payloads or reasons.
use crate::auth::Auth;
use aws_sdk_cloudwatch::{operation::describe_alarms::DescribeAlarmsOutput, types::AlarmType};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{
    projection::{observation, operation},
    transport::Error,
};
use tokio_util::sync::CancellationToken;

pub async fn collect(auth: &Auth, job: &Job, cancel: &CancellationToken) -> CheckResult {
    let mut result = crate::metrics::empty(job);
    let Auth::Aws(clients) = auth else {
        return result;
    };
    result.operations.clear();
    for region in &job.target.regions {
        let id = format!("cloudwatch-alarms/{region}");
        let mut pages = 0;
        let mut outcome = Ok(0usize);
        let client = match clients.cloudwatch(region, &job.settings).await {
            Ok(client) => client,
            Err(error) => {
                result.operations.push(operation(&id, Err(&error), 0, true));
                continue;
            }
        };
        let mut token = None;
        for page in 0..job.settings.max_pages {
            pages = page + 1;
            let response = tokio::select! {
                _=cancel.cancelled()=>Err(Error::Cancelled),
                response=client.describe_alarms().alarm_types(AlarmType::MetricAlarm).alarm_types(AlarmType::CompositeAlarm).alarm_types(AlarmType::LogAlarm).max_records(job.settings.page_size.min(100) as i32).set_next_token(token.clone()).send()=>response.map_err(crate::aws_errors::sdk),
            };
            match response {
                Ok(response) => {
                    let projected = project(job, &id, &response);
                    let remaining = job
                        .settings
                        .max_assets
                        .saturating_sub(result.observations.len());
                    let count = projected.len();
                    result
                        .observations
                        .extend(projected.into_iter().take(remaining));
                    outcome = Ok(result.observations.len());
                    if count > remaining {
                        outcome = Err(Error::Limit);
                        break;
                    }
                    let next = response
                        .next_token()
                        .filter(|token| !token.is_empty())
                        .map(String::from);
                    if next.is_none() {
                        break;
                    }
                    if next == token || page + 1 == job.settings.max_pages {
                        outcome = Err(Error::Limit);
                        break;
                    }
                    token = next;
                }
                Err(error) => {
                    outcome = Err(error);
                    break;
                }
            }
        }
        result.operations.push(operation(
            &id,
            outcome.as_ref().copied(),
            pages,
            job.settings.required,
        ));
    }
    result.finished_at = chrono::Utc::now();
    result
}
pub fn project(job: &Job, id: &str, response: &DescribeAlarmsOutput) -> Vec<Observation> {
    let metric = response.metric_alarms().iter().map(|alarm| {
        (
            alarm.alarm_arn().or(alarm.alarm_name()),
            alarm.state_value(),
        )
    });
    let composite = response.composite_alarms().iter().map(|alarm| {
        (
            alarm.alarm_arn().or(alarm.alarm_name()),
            alarm.state_value(),
        )
    });
    let logs = response.log_alarms().iter().map(|alarm| {
        (
            alarm.alarm_arn().or(alarm.alarm_name()),
            alarm.state_value(),
        )
    });
    metric
        .chain(composite)
        .chain(logs)
        .filter_map(|(name, state)| {
            let name = name?;
            if !job.target.resources.is_empty()
                && !job
                    .target
                    .resources
                    .iter()
                    .any(|selector| name.contains(selector))
            {
                return None;
            }
            Some(observation(
                job,
                id,
                name,
                Data::Condition {
                    rule: "cloudwatch-alarm".into(),
                    healthy: state.and_then(|state| match state.as_str() {
                        "OK" => Some(true),
                        "ALARM" => Some(false),
                        _ => None,
                    }),
                },
            ))
        })
        .collect()
}

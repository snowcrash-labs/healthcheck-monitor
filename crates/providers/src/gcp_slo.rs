//! Read configured Google SLO compliance, budgets, and optional burn-rate telemetry.
use crate::common::{Endpoint, Source};
use chrono::{DateTime, Duration, Utc};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{
    projection::{observation, operation},
    transport::Error,
};
use tokio_util::sync::CancellationToken;
pub async fn collect_from<S: Source>(
    source: &S,
    job: &Job,
    cancel: &CancellationToken,
) -> CheckResult {
    let mut result =
        crate::common::collect_from(source, job, crate::gcp::endpoints(job), cancel).await;
    let definitions: Vec<_> = result
        .observations
        .iter()
        .filter_map(|obs| match &obs.data {
            Data::SloDefinition {
                name,
                goal,
                period_seconds,
            } => Some((name.clone(), *goal, *period_seconds)),
            _ => None,
        })
        .collect();
    for (index, (name, goal, period_seconds)) in definitions.into_iter().enumerate() {
        let id = format!(
            "slo/{}",
            crate::metric_window::id(&serde_json::Value::String(name.clone()))
        );
        if index >= job.settings.max_series {
            result
                .operations
                .push(operation("slo-series-limit", Err(&Error::Limit), 0, true));
            break;
        }
        let Some(goal) = goal else {
            result
                .operations
                .push(operation(&id, Err(&Error::Malformed), 0, true));
            continue;
        };
        let mut compliance = None;
        let mut budget = None;
        let mut burn_rate = None;
        let mut at = None;
        let mut status = Ok(0);
        for selector in ["compliance", "budget", "burn_rate"] {
            if selector == "burn_rate" && job.settings.slo_burn_rate_error.is_none() {
                continue;
            }
            match series(source, job, &name, selector, cancel).await {
                Ok((observed, value)) => {
                    at = Some(at.map_or(observed, |at: DateTime<Utc>| at.min(observed)));
                    match selector {
                        "compliance" => compliance = Some(value),
                        "budget" => budget = Some(value),
                        _ => burn_rate = Some(value),
                    }
                }
                Err(error) => status = Err(error),
            }
        }
        if let Some(at) = at {
            let mut obs = observation(
                job,
                &id,
                &name,
                Data::Slo {
                    goal,
                    compliance,
                    budget,
                    burn_rate,
                    period_seconds,
                },
            );
            obs.observed_at = at;
            result.observations.push(obs);
        }
        if compliance.is_some_and(|value| !(0.0..=1.0).contains(&value)) {
            status = Err(Error::Malformed);
        }
        result.operations.push(operation(
            &id,
            status.map(|_| 1).as_ref().copied(),
            1,
            job.settings.required,
        ));
    }
    result.finished_at = Utc::now();
    result
}
async fn series<S: Source>(
    source: &S,
    job: &Job,
    name: &str,
    selector: &str,
    cancel: &CancellationToken,
) -> Result<(DateTime<Utc>, f64), Error> {
    let now = Utc::now();
    let mut url = url::Url::parse(&format!(
        "https://monitoring.googleapis.com/v3/projects/{}/timeSeries",
        job.target.scope
    ))
    .map_err(|_| Error::Malformed)?;
    let quoted = serde_json::to_string(name).map_err(|_| Error::Malformed)?;
    let filter = if selector == "burn_rate" {
        format!(
            "select_slo_burn_rate({quoted}, \"{}s\")",
            job.settings.slo_burn_window.0
        )
    } else {
        format!("select_slo_{selector}({quoted})")
    };
    url.query_pairs_mut()
        .append_pair("filter", &filter)
        .append_pair(
            "interval.startTime",
            &(now - Duration::seconds(job.settings.metric_window.0 as i64)).to_rfc3339(),
        )
        .append_pair("interval.endTime", &now.to_rfc3339())
        .append_pair("pageSize", "10");
    let endpoint = Endpoint::get(format!("slo-{selector}"), url.as_str(), "/timeSeries");
    let value = source.request(&endpoint, job, cancel).await?;
    let response: google_cloud_monitoring_v3::model::ListTimeSeriesResponse =
        serde_json::from_value(value).map_err(|_| Error::Malformed)?;
    if !response.next_page_token.is_empty() {
        return Err(Error::Limit);
    }
    if response.time_series.len() > 1 {
        return Err(Error::Malformed);
    }
    response
        .time_series
        .iter()
        .flat_map(|series| &series.points)
        .filter_map(|point| {
            let at =
                DateTime::from_timestamp(point.interval.as_ref()?.end_time.as_ref()?.seconds(), 0)?;
            let value = *point.value.as_ref()?.double_value()?;
            value.is_finite().then_some((at, value))
        })
        .max_by_key(|(at, _)| *at)
        .ok_or(Error::Missing)
}

//! Native Application Signals SLO inventory and batches of budget reports.
use crate::auth::Auth;
use aws_sdk_applicationsignals::{
    error::ProvideErrorMetadata,
    operation::batch_get_service_level_objective_budget_report::BatchGetServiceLevelObjectiveBudgetReportOutput,
};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{
    projection::{observation, operation},
    transport::Error,
};
use tokio_util::sync::CancellationToken;
pub async fn collect(auth: &Auth, job: &Job, cancel: &CancellationToken) -> CheckResult {
    let mut result = crate::router::base(job);
    let Auth::Aws(clients) = auth else {
        return CheckResult::failure(
            job.target.name.clone(),
            job.check,
            job.revision.clone(),
            Coverage::Unauthenticated,
        );
    };
    let mut total = 0;
    for region in &job.target.regions {
        let id = format!("slo-inventory/{region}");
        let client = match clients.signals(region).await {
            Ok(client) => client,
            Err(error) => {
                result.operations.push(operation(&id, Err(&error), 0, true));
                continue;
            }
        };
        let mut ids = Vec::new();
        let mut token = None;
        let mut status = Ok(());
        let mut pages = 0;
        for page in 0..job.settings.max_pages {
            pages = page + 1;
            let response = tokio::select! {
                _=cancel.cancelled()=>Err(Error::Cancelled),
                response=client.list_service_level_objectives().include_linked_accounts(false).max_results(job.settings.page_size.min(50) as i32).set_next_token(token.clone()).send()=>response.map_err(|error|crate::aws_errors::classify(error.as_service_error().and_then(|error|error.code()))),
            };
            match response {
                Ok(response) => {
                    for slo in response.slo_summaries() {
                        let arn = slo.arn();
                        if !owned(arn, region, &job.target.scope) {
                            status = Err(Error::Forbidden);
                            continue;
                        }
                        if !job.target.resources.is_empty()
                            && !job
                                .target
                                .resources
                                .iter()
                                .any(|selector| arn.contains(selector))
                        {
                            continue;
                        }
                        if total >= job.settings.max_series {
                            status = Err(Error::Limit);
                            break;
                        }
                        ids.push(arn.to_string());
                        total += 1;
                    }
                    let next = response
                        .next_token()
                        .filter(|token| !token.is_empty())
                        .map(String::from);
                    if status.is_err() || next.is_none() {
                        break;
                    }
                    if next == token || pages == job.settings.max_pages {
                        status = Err(Error::Limit);
                        break;
                    }
                    token = next;
                }
                Err(error) => {
                    status = Err(error);
                    break;
                }
            }
        }
        result.operations.push(operation(
            &id,
            status.map(|()| ids.len()).as_ref().copied(),
            pages,
            job.settings.required,
        ));
        for batch in ids.chunks(50) {
            let response = tokio::select! {
                _=cancel.cancelled()=>Err(Error::Cancelled),
                response=client.batch_get_service_level_objective_budget_report().set_slo_ids(Some(batch.to_vec())).timestamp(aws_smithy_types::DateTime::from_secs(chrono::Utc::now().timestamp())).send()=>response.map_err(|error|crate::aws_errors::classify(error.as_service_error().and_then(|error|error.code()))),
            };
            match response {
                Ok(response) => project(job, region, batch, &response, &mut result),
                Err(error) => {
                    for arn in batch {
                        result.operations.push(operation(
                            &format!("slo/{arn}"),
                            Err(&error),
                            1,
                            job.settings.required,
                        ));
                    }
                }
            }
        }
    }
    result.finished_at = chrono::Utc::now();
    result
}
fn owned(arn: &str, region: &str, account: &str) -> bool {
    let parts: Vec<_> = arn.splitn(6, ':').collect();
    parts.len() == 6
        && matches!(parts[1], "aws" | "aws-us-gov")
        && parts[2] == "application-signals"
        && parts[3] == region
        && parts[4] == account
        && parts[5].starts_with("slo/")
}
pub fn project(
    job: &Job,
    region: &str,
    requested: &[String],
    response: &BatchGetServiceLevelObjectiveBudgetReportOutput,
    result: &mut CheckResult,
) {
    let at = chrono::DateTime::from_timestamp(response.timestamp().secs(), 0);
    for arn in requested {
        let id = format!("slo/{arn}");
        let outcome = (|| {
            if !owned(arn, region, &job.target.scope) {
                return Err(Error::Forbidden);
            }
            let at = at.ok_or(Error::Malformed)?;
            let row = response
                .reports()
                .iter()
                .find(|row| row.arn() == arn)
                .ok_or_else(|| {
                    response
                        .errors()
                        .iter()
                        .find(|error| error.arn() == arn)
                        .map_or(Error::Missing, |error| {
                            crate::aws_errors::classify(Some(error.error_code()))
                        })
                })?;
            let goal = row
                .goal()
                .and_then(|goal| goal.attainment_goal())
                .filter(|goal| goal.is_finite() && *goal > 0.0 && *goal <= 100.0)
                .ok_or(Error::Malformed)?
                / 100.0;
            let compliance = row
                .attainment()
                .filter(|value| value.is_finite() && (0.0..=100.0).contains(value))
                .map(|value| value / 100.0);
            let budget = row
                .budget_requests_remaining()
                .or(row.budget_seconds_remaining())
                .map(f64::from);
            let mut obs = observation(
                job,
                &id,
                arn,
                Data::Slo {
                    goal,
                    compliance,
                    budget,
                    burn_rate: None,
                    period_seconds: None,
                },
            );
            obs.observed_at = at;
            result.observations.push(obs);
            if compliance.is_none() || row.budget_status().as_str() == "INSUFFICIENT_DATA" {
                Err(Error::Missing)
            } else {
                Ok(1)
            }
        })();
        result.operations.push(operation(
            &id,
            outcome.as_ref().copied(),
            1,
            job.settings.required,
        ));
    }
}

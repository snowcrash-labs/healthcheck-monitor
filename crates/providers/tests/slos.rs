//! Native and typed REST SLO evidence keeps failures and missing metrics separate.
use chrono::Utc;
use monitor_core::{
    config::{
        resolve::{Job, Selection},
        types::Config,
    },
    model::*,
};
use monitor_integrations::transport::Error;
use monitor_providers::{
    common::{Endpoint, Source},
    gcp_slo,
};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex};
use tokio_util::sync::CancellationToken;
struct Fake {
    values: Mutex<VecDeque<Result<Value, Error>>>,
    urls: Mutex<Vec<String>>,
}
impl Source for Fake {
    async fn request(
        &self,
        endpoint: &Endpoint,
        _: &Job,
        _: &CancellationToken,
    ) -> Result<Value, Error> {
        self.urls
            .lock()
            .map_err(|_| Error::Unavailable)?
            .push(endpoint.url.clone());
        self.values
            .lock()
            .map_err(|_| Error::Unavailable)?
            .pop_front()
            .ok_or(Error::Missing)?
    }
}
fn job(provider: &str) -> Result<Job, Box<dyn std::error::Error>> {
    Config::parse(&format!("version=1\n[[targets]]\nname='test'\nprovider='{provider}'\nscope='123456789012'\nregions=['us-east-1']"))?.resolve(&Selection::default())?.jobs.into_iter().find(|job|job.check==Check::Slo).ok_or_else(||"missing SLO job".into())
}
fn series(value: f64) -> Value {
    json!({"timeSeries":[{"points":[{"interval":{"endTime":Utc::now().to_rfc3339()},"value":{"doubleValue":value}}]}]})
}
fn source(budget: Result<Value, Error>) -> Fake {
    Fake {
        values: Mutex::new(VecDeque::from([
            Ok(json!({"services":[{"name":"projects/123456789012/services/api"}]})),
            Ok(
                json!({"serviceLevelObjectives":[{"name":"projects/123456789012/services/api/serviceLevelObjectives/availability","goal":0.999,"rollingPeriod":"86400s"}]}),
            ),
            Ok(series(0.98)),
            budget,
        ])),
        urls: Mutex::new(Vec::new()),
    }
}
#[tokio::test]
async fn google_slo_definitions_lead_to_actual_compliance_and_budget_reads()
-> Result<(), Box<dyn std::error::Error>> {
    let source = source(Ok(series(-20.0)));
    let job = job("gcp")?;
    let result = gcp_slo::collect_from(&source, &job, &CancellationToken::new()).await;
    assert!(result.complete());
    assert!(result.observations.iter().any(|obs| matches!(
        obs.data,
        Data::Slo {
            goal: 0.999,
            compliance: Some(0.98),
            budget: Some(-20.0),
            period_seconds: Some(86400),
            ..
        }
    )));
    let urls = source.urls.lock().map_err(|_| "lock")?;
    assert!(urls.iter().any(|url| url.contains("select_slo_compliance")));
    assert!(urls.iter().any(|url| url.contains("select_slo_budget")));
    Ok(())
}
#[tokio::test]
async fn missing_budget_does_not_discard_available_compliance()
-> Result<(), Box<dyn std::error::Error>> {
    let result = gcp_slo::collect_from(
        &source(Err(Error::Denied)),
        &job("gcp")?,
        &CancellationToken::new(),
    )
    .await;
    assert!(!result.complete());
    assert!(result.observations.iter().any(|obs| matches!(
        obs.data,
        Data::Slo {
            compliance: Some(0.98),
            budget: None,
            ..
        }
    )));
    Ok(())
}
#[test]
fn aws_budget_reports_preserve_provider_time_and_per_objective_coverage()
-> Result<(), Box<dyn std::error::Error>> {
    use aws_sdk_applicationsignals::{
        operation::batch_get_service_level_objective_budget_report::BatchGetServiceLevelObjectiveBudgetReportOutput,
        types::{Goal, ServiceLevelObjectiveBudgetReport, ServiceLevelObjectiveBudgetStatus},
    };
    let job = job("aws")?;
    let arn = "arn:aws:application-signals:us-east-1:123456789012:slo/api";
    let report = ServiceLevelObjectiveBudgetReport::builder()
        .arn(arn)
        .name("api")
        .budget_status(ServiceLevelObjectiveBudgetStatus::Breached)
        .attainment(98.0)
        .budget_seconds_remaining(-10)
        .goal(Goal::builder().attainment_goal(99.9).build())
        .build()?;
    let response = BatchGetServiceLevelObjectiveBudgetReportOutput::builder()
        .timestamp(aws_smithy_types::DateTime::from_secs(1800000000))
        .reports(report)
        .set_errors(Some(vec![]))
        .build()?;
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    result.operations.clear();
    monitor_providers::aws_slo::project(
        &job,
        "us-east-1",
        &[arn.into(), format!("{arn}-missing")],
        &response,
        &mut result,
    );
    assert_eq!(result.observations.len(), 1);
    assert_eq!(result.observations[0].observed_at.timestamp(), 1800000000);
    assert!(matches!(
        result.observations[0].data,
        Data::Slo {
            compliance: Some(0.98),
            ..
        }
    ));
    assert!(!result.complete());
    Ok(())
}

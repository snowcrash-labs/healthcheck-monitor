//! Pipeline execution, distribution state and quota registration have operational projections.
use monitor_core::{
    config::{
        resolve::{Job, Selection},
        types::Config,
    },
    model::*,
};
use monitor_providers::{common::Endpoint, details::followups, resource_projection::project};
use serde_json::json;
fn job() -> Result<Job, Box<dyn std::error::Error>> {
    Config::parse("version=1\n[[targets]]\nname='test'\nprovider='aws'\nscope='123456789012'\nregions=['us-east-1']\n[targets.build_targets]\nrelease='api'")?.resolve(&Selection::default())?.jobs.into_iter().find(|job|job.check==Check::Releases).ok_or_else(||"missing job".into())
}
#[test]
fn pipeline_execution_failure_is_a_deployment_warning_and_excludes_payloads()
-> Result<(), Box<dyn std::error::Error>> {
    let job = job()?;
    let mut endpoint = Endpoint::get(
        "pipeline-state/us-east-1/release",
        "https://codepipeline.us-east-1.amazonaws.com/",
        "",
    );
    endpoint.aws = Some((
        "codepipeline".into(),
        "us-east-1".into(),
        "CodePipeline_20150709.GetPipelineState".into(),
    ));
    let reads = followups(&job, &endpoint, &json!({"pipelineName":"release"}));
    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0].items, "/pipelineExecutionSummaries");
    let value = json!({"pipelineExecutionId":"execution","status":"Failed","startTime":chrono::Utc::now().timestamp(),"sourceRevisions":[{"revisionId":"abcdef1234567890","revisionSummary":"private-customer"}],"statusSummary":"private-failure","variables":[{"resolvedValue":"private-value"}]});
    let observations = project(&job, &reads[0], &value);
    let policy =
        monitor_core::policy::evaluate(&observations[0], None, &job.settings, chrono::Utc::now());
    assert_eq!(policy.health, Health::Degraded);
    assert!(
        matches!(&observations[0].data,Data::Build{pipeline,revision,target,..} if pipeline=="release" && revision=="abcdef1234567890" && target=="api")
    );
    assert!(!serde_json::to_string(&observations)?.contains("private-"));
    Ok(())
}
#[test]
fn distribution_state_and_resource_selection_do_not_fabricate_missing_resources()
-> Result<(), Box<dyn std::error::Error>> {
    let mut job = job()?;
    let distribution = Endpoint::get(
        "cloudfront",
        "https://cloudfront.amazonaws.com/2020-05-31/distribution",
        "",
    );
    assert!(matches!(
        project(
            &job,
            &distribution,
            &json!({"Id":"dist","Enabled":true,"Status":"Deployed"})
        )[0]
        .data,
        Data::Service {
            state: ServiceState::Ready,
            ..
        }
    ));
    job.target.resources = vec!["database".into()];
    let rds = Endpoint::get("rds/us-east-1", "https://rds.us-east-1.amazonaws.com/", "");
    assert!(
        project(
            &job,
            &rds,
            &json!({"DBInstanceIdentifier":"database","DBInstanceStatus":"available"})
        )
        .iter()
        .any(|observation| matches!(
            observation.data,
            Data::Service {
                state: ServiceState::Ready,
                ..
            }
        ))
    );
    Ok(())
}
#[test]
fn quotas_follow_discovered_service_codes_instead_of_only_compute()
-> Result<(), Box<dyn std::error::Error>> {
    let job = job()?;
    let mut parent = Endpoint::get(
        "quota-services/us-east-1",
        "https://servicequotas.us-east-1.amazonaws.com/",
        "/Services",
    );
    parent.aws = Some((
        "servicequotas".into(),
        "us-east-1".into(),
        "ServiceQuotasV20190624.ListServices".into(),
    ));
    let reads = followups(
        &job,
        &parent,
        &json!({"ServiceCode":"lambda","ServiceName":"AWS Lambda"}),
    );
    assert_eq!(reads.len(), 1);
    assert_eq!(
        reads[0]
            .body
            .as_ref()
            .and_then(|body| body.get("ServiceCode")),
        Some(&json!("lambda"))
    );
    Ok(())
}

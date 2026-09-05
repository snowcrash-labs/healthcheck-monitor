//! Managed runtimes supply image metadata without retaining configuration or download tokens.
use monitor_core::{
    config::{resolve::Selection, types::Config},
    model::*,
};
use monitor_providers::{common::Endpoint, resource_projection::project};
use serde_json::json;
fn job() -> Result<monitor_core::config::resolve::Job, Box<dyn std::error::Error>> {
    Config::parse("version=1\n[[targets]]\nname='dev'\nprovider='aws'\nscope='123456789012'\nregions=['us-east-1']")?.resolve(&Selection::default())?.jobs.into_iter().find(|job|job.check==Check::Releases).ok_or_else(||"job".into())
}
#[test]
fn ecs_tasks_expose_resolved_digests_without_container_overrides()
-> Result<(), Box<dyn std::error::Error>> {
    let digest = format!("sha256:{}", "a".repeat(64));
    let endpoint = Endpoint::get(
        "ecs-tasks-detail/us-east-1/task",
        "https://ecs.us-east-1.amazonaws.com/",
        "/tasks",
    );
    let observations = project(
        &job()?,
        &endpoint,
        &json!({"taskArn":"task","lastStatus":"RUNNING","desiredStatus":"RUNNING","containers":[{"name":"app","image":"registry.example/app:release-abcdef0","imageDigest":digest}],"overrides":{"containerOverrides":[{"environment":[{"name":"SECRET","value":"private-environment"}]}]}}),
    );
    assert!(observations.iter().any(|obs| matches!(
        obs.data,
        Data::Image {
            observed_digest: Some(_),
            ..
        }
    )));
    assert!(
        observations
            .iter()
            .any(|obs| matches!(obs.data, Data::Workload { ready: 1, .. }))
    );
    assert!(!serde_json::to_string(&observations)?.contains("private-environment"));
    Ok(())
}
#[test]
fn lambda_image_provenance_does_not_retain_package_download_urls()
-> Result<(), Box<dyn std::error::Error>> {
    let digest = format!("sha256:{}", "b".repeat(64));
    let endpoint = Endpoint::get(
        "lambda-image/us-east-1/function",
        "https://lambda.us-east-1.amazonaws.com/2015-03-31/functions/function",
        "",
    );
    let observations = project(
        &job()?,
        &endpoint,
        &json!({"Configuration":{"FunctionName":"function","State":"Active","Environment":{"Variables":{"TOKEN":"private-token"}}},"Code":{"ImageUri":"registry.example/function:release-abcdef0","ResolvedImageUri":format!("registry.example/function@{digest}"),"Location":"https://download.invalid/private-token"}}),
    );
    assert!(observations.iter().any(|obs| matches!(
        obs.data,
        Data::Image {
            observed_digest: Some(_),
            ..
        }
    )));
    assert!(!serde_json::to_string(&observations)?.contains("private-token"));
    Ok(())
}

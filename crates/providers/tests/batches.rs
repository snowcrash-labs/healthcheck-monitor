//! Batched provider reads retain individual outcomes and stable resource identities.
use monitor_core::{
    config::{resolve::Selection, types::Config},
    model::*,
};
use monitor_integrations::transport::Error;
use monitor_providers::common::{Endpoint, Source, collect_from};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_util::sync::CancellationToken;
struct Fake(AtomicUsize);
impl Source for Fake {
    async fn request(
        &self,
        endpoint: &Endpoint,
        _: &monitor_core::config::resolve::Job,
        _: &CancellationToken,
    ) -> Result<Value, Error> {
        self.0.fetch_add(1, Ordering::SeqCst);
        if endpoint.id.starts_with("codebuild/") {
            return Ok(
                json!({"ids":(0..201).map(|index|format!("build-{index}")).collect::<Vec<_>>()}),
            );
        }
        let ids = endpoint
            .body
            .as_ref()
            .and_then(|body| body.get("ids"))
            .and_then(Value::as_array)
            .ok_or(Error::Malformed)?;
        assert!(ids.len() <= 100);
        Ok(
            json!({"builds":ids.iter().filter(|id|id.as_str()!=Some("build-3")).map(|id|json!({"id":id,"projectName":"pipeline","buildStatus":"SUCCEEDED"})).collect::<Vec<_>>(),"buildsNotFound":["build-3"]}),
        )
    }
}
#[tokio::test]
async fn build_metadata_is_batched_and_a_missing_build_does_not_discard_others()
-> Result<(), Box<dyn std::error::Error>> {
    let job=Config::parse("version=1\n[[targets]]\nname='dev'\nprovider='aws'\nscope='123456789012'\nregions=['us-east-1']")?.resolve(&Selection::default())?.jobs.into_iter().find(|job|job.check==Check::Releases).ok_or("job")?;
    let source = Fake(AtomicUsize::new(0));
    let mut endpoint = Endpoint::get(
        "codebuild/us-east-1",
        "https://codebuild.us-east-1.amazonaws.com/",
        "/ids",
    );
    endpoint.aws = Some((
        "codebuild".into(),
        "us-east-1".into(),
        "CodeBuild_20161006.ListBuilds".into(),
    ));
    endpoint.body = Some(json!({}));
    let result = collect_from(&source, &job, vec![endpoint], &CancellationToken::new()).await;
    assert_eq!(source.0.load(Ordering::SeqCst), 4);
    assert_eq!(
        result
            .observations
            .iter()
            .filter(|obs| matches!(obs.data, Data::Build { .. }))
            .count(),
        200
    );
    assert!(result.operations.iter().any(|operation| operation.id
        == "build-details/us-east-1/build-3"
        && operation.coverage == Coverage::Missing));
    assert!(
        result
            .observations
            .iter()
            .any(|obs| obs.resource == "dev/build-details/us-east-1/build-4/build-4")
    );
    Ok(())
}

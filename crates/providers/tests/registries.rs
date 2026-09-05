//! Registry metadata preserves platform relationships and rejects unsafe pagination.
use monitor_core::{
    config::{
        resolve::{Job, Selection},
        types::Config,
    },
    model::*,
};
use monitor_integrations::transport::Error;
use monitor_providers::{
    azure_registry,
    common::{Endpoint, Source, collect_from},
};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex};
use tokio_util::sync::CancellationToken;
struct Fake(Mutex<VecDeque<Value>>);
impl Source for Fake {
    async fn request(&self, _: &Endpoint, _: &Job, _: &CancellationToken) -> Result<Value, Error> {
        self.0
            .lock()
            .map_err(|_| Error::Unavailable)?
            .pop_front()
            .ok_or(Error::Missing)
    }
}
fn job(provider: &str) -> Result<Job, Box<dyn std::error::Error>> {
    Config::parse(&format!("version=1\n[[targets]]\nname='target'\nprovider='{provider}'\nscope='project'\nregions=['us-central1']"))?.resolve(&Selection::default())?.jobs.into_iter().find(|job|job.check==Check::Releases).ok_or_else(||"job".into())
}
#[tokio::test]
async fn google_index_metadata_produces_child_relationships()
-> Result<(), Box<dyn std::error::Error>> {
    let digest = format!("sha256:{}", "a".repeat(64));
    let child = format!("sha256:{}", "b".repeat(64));
    let source = Fake(Mutex::new(VecDeque::from([
        json!({"dockerImages":[{"uri":format!("us-docker.pkg.dev/project/repo/app@{digest}"),"mediaType":"application/vnd.oci.image.index.v1+json"}]}),
        json!({"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json","manifests":[{"digest":child}],"private":"forbidden"}),
    ])));
    let result = collect_from(
        &source,
        &job("gcp")?,
        vec![Endpoint::get(
            "registry-images/repo",
            "https://artifactregistry.googleapis.com/v1/images",
            "/dockerImages",
        )],
        &CancellationToken::new(),
    )
    .await;
    assert!(result.complete());
    assert!(
        result
            .observations
            .iter()
            .any(|obs| matches!(&obs.data,Data::Artifact{children,..}if children.len()==1))
    );
    assert!(!serde_json::to_string(&result)?.contains("forbidden"));
    Ok(())
}
fn base(job: &Job) -> CheckResult {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Complete,
    );
    result.observations.push(Observation {
        resource: "target/registry".into(),
        operation: "Releases".into(),
        observed_at: chrono::Utc::now(),
        expected: Expected::Active,
        data: Data::Registry {
            host: "registry.azurecr.io".into(),
        },
    });
    result
}
#[tokio::test]
async fn azure_manifest_metadata_has_tags_and_platform_references()
-> Result<(), Box<dyn std::error::Error>> {
    let job = job("azure")?;
    let mut result = base(&job);
    let digest = format!("sha256:{}", "a".repeat(64));
    let child = format!("sha256:{}", "b".repeat(64));
    let source = Fake(Mutex::new(VecDeque::from([
        json!({"repositories":["app"]}),
        json!({"manifests":[{"digest":digest,"tags":["release-abcdef0"],"references":[{"digest":child,"os":"linux"}],"customer":"forbidden"}]}),
    ])));
    azure_registry::enrich(&source, &job, &mut result, &CancellationToken::new()).await;
    assert!(result.complete());
    assert!(result.observations.iter().any(|obs|matches!(&obs.data,Data::Artifact{manifest:ManifestKind::Index,children,tags,..}if children.len()==1 && tags.len()==1)));
    assert!(!serde_json::to_string(&result)?.contains("forbidden"));
    Ok(())
}
#[tokio::test]
async fn registry_pagination_cannot_send_credentials_to_a_different_origin()
-> Result<(), Box<dyn std::error::Error>> {
    let job = job("azure")?;
    let mut result = base(&job);
    let source = Fake(Mutex::new(VecDeque::from([
        json!({"repositories":[],"_monitor_next":"https://other.azurecr.io/v2/_catalog"}),
    ])));
    azure_registry::enrich(&source, &job, &mut result, &CancellationToken::new()).await;
    assert!(
        result
            .operations
            .iter()
            .any(|operation| operation.coverage == Coverage::Denied)
    );
    Ok(())
}

//! Registry digests and successful build revisions are projected independently.
use monitor_core::{
    config::{resolve::Selection, types::Config},
    model::*,
};
use monitor_providers::{common::Endpoint, resource_projection::project};
use serde_json::json;
#[test]
fn registry_tags_are_not_treated_as_build_attestations() -> Result<(), Box<dyn std::error::Error>> {
    let job = Config::parse("version=1\n[[targets]]\nname='dev'\nprovider='gcp'\nscope='project'")?
        .resolve(&Selection::default())?
        .jobs
        .into_iter()
        .find(|job| job.check == Check::Releases)
        .ok_or("job")?;
    let digest = format!("sha256:{}", "a".repeat(64));
    let endpoint = Endpoint::get(
        "registry-images/repo",
        "https://artifactregistry.googleapis.com/v1/images",
        "/dockerImages",
    );
    let registry = project(
        &job,
        &endpoint,
        &json!({"uri":format!("us-docker.pkg.dev/project/repo/app@{digest}"),"mediaType":"application/vnd.oci.image.manifest.v1+json","tags":["release-abcdef0"],"private":"forbidden"}),
    );
    assert!(matches!(
        registry[0].data,
        Data::Artifact {
            built: false,
            revision: None,
            manifest: ManifestKind::Image,
            ..
        }
    ));
    let endpoint = Endpoint::get(
        "builds/global",
        "https://cloudbuild.googleapis.com/v1/builds",
        "/builds",
    );
    let revision = "b".repeat(40);
    let build = project(
        &job,
        &endpoint,
        &json!({"id":"build","status":"SUCCESS","substitutions":{"COMMIT_SHA":revision,"REPO_FULL_NAME":"org/app"},"results":{"images":[{"name":"us-docker.pkg.dev/project/repo/app:release-abcdef0","digest":digest}]},"environment":"forbidden"}),
    );
    assert!(build.iter().any(|obs|matches!(&obs.data,Data::Artifact{built:true,revision:Some(revision),repository:Some(repository),..}if revision.len()==40 && repository=="org/app")));
    assert!(!serde_json::to_string(&(registry, build))?.contains("forbidden"));
    Ok(())
}

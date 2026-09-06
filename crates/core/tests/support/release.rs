//! Provenance requires independent registry, build, and repository evidence.
use chrono::{DateTime, Duration, Utc};
use monitor_core::{
    config::{
        resolve::{Job, Selection},
        types::Config,
    },
    model::*,
    state::State,
};
pub fn job() -> Result<Job, Box<dyn std::error::Error>> {
    Config::parse(
        "version=1\n[[targets]]\nname='dev'\nprovider='gcp'\nscope='project'\ncontext='cluster'",
    )?
    .resolve(&Selection::default())?
    .jobs
    .into_iter()
    .find(|job| job.check == Check::Releases)
    .ok_or_else(|| "job".into())
}
pub fn observation(resource: &str, operation: &str, data: Data, at: DateTime<Utc>) -> Observation {
    Observation {
        context: None,
        resource: resource.into(),
        operation: operation.into(),
        observed_at: at,
        expected: Expected::Active,
        data,
    }
}
pub fn result(
    target: &str,
    check: Check,
    observations: Vec<Observation>,
    at: DateTime<Utc>,
) -> CheckResult {
    let ids: std::collections::BTreeSet<_> = observations
        .iter()
        .map(|obs| obs.operation.clone())
        .collect();
    CheckResult {
        target: target.into(),
        check,
        revision: "revision".into(),
        started_at: at,
        finished_at: at,
        operations: ids
            .into_iter()
            .map(|id| Operation {
                id,
                coverage: Coverage::Complete,
                observed_at: at,
                records: 1,
                pages: 1,
                attempts: 1,
                required: true,
            })
            .collect(),
        observations,
    }
}
pub type Fixture = (Job, Snapshot, CheckResult, DateTime<Utc>);
pub fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let job = job()?;
    let now = Utc::now();
    let digest = format!("sha256:{}", "b".repeat(64));
    let revision = "a".repeat(40);
    let image = "us-docker.pkg.dev/project/repo/app";
    let mut snapshot = State::new(job.revision.clone(), vec![]).snapshot;
    snapshot.results.insert(
        "dev/Kubernetes".into(),
        result(
            "dev",
            Check::Kubernetes,
            vec![observation(
                "dev/pods/ns/app/image/app",
                "pods",
                Data::Image {
                    desired: format!("{image}:release-aaaaaaa"),
                    observed_digest: Some(digest.clone()),
                    revision: Some("aaaaaaa".into()),
                },
                now,
            )],
            now,
        ),
    );
    snapshot.results.insert(
        "source/Github".into(),
        result(
            "source",
            Check::Github,
            vec![observation(
                "source/commit",
                "commits",
                Data::Commit {
                    repository: "org/app".into(),
                    revision: revision.clone(),
                    reference: Some("main".into()),
                },
                now,
            )],
            now,
        ),
    );
    let artifact = |built| Data::Artifact {
        manifest: if built {
            ManifestKind::Unknown
        } else {
            ManifestKind::Image
        },
        children: vec![],
        image: image.into(),
        digest: digest.clone(),
        tags: vec!["release-aaaaaaa".into()],
        revision: built.then(|| revision.clone()),
        repository: built.then(|| "org/app".into()),
        built,
        created_at: Some(now - Duration::hours(1)),
    };
    let registry = artifact(false);
    let build = artifact(true);
    let raw = result(
        "dev",
        Check::Releases,
        vec![
            observation("dev/registry", "registry", registry, now),
            observation("dev/build", "builds", build, now),
        ],
        now,
    );
    Ok((job, snapshot, raw, now))
}
pub fn trace(result: &CheckResult) -> Option<&Observation> {
    result
        .observations
        .iter()
        .find(|obs| matches!(obs.data, Data::Provenance { .. }))
}

//! Correlate runtime images with registry, build, and repository evidence.
use crate::{config::resolve::Job, images, model::*, release_evidence::Evidence};
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
pub fn enrich(snapshot: &Snapshot, job: &Job, current: &mut CheckResult, now: DateTime<Utc>) {
    current
        .operations
        .retain(|op| !op.id.starts_with("provenance/"));
    current
        .observations
        .retain(|obs| !matches!(obs.data, Data::Provenance { .. }));
    let evidence = Evidence::new(snapshot, job, current, now);
    let mut observations = Vec::new();
    let mut operations = Vec::new();
    for image in evidence.images(&job.target.name).into_iter().take(
        job.settings
            .max_assets
            .saturating_sub(current.observations.len()),
    ) {
        let Data::Image {
            desired,
            observed_digest,
            revision,
        } = &image.data
        else {
            continue;
        };
        let id = format!("provenance/{}", image.resource);
        let Some(canonical) = images::canonical(desired) else {
            operations.push(operation(id, Coverage::Malformed, now, true));
            continue;
        };
        let tracked = revision.is_some()
            || canonical.contains(&job.target.scope)
            || job
                .target
                .build_targets
                .values()
                .any(|target| canonical.ends_with(&format!("/{target}")))
            || evidence.artifacts.contains_key(canonical.as_str());
        let inactive = evidence.inactive(image);
        if !tracked && !inactive {
            operations.push(operation(id, Coverage::InventoryOnly, now, false));
            continue;
        }
        let mut runtime = if observed_digest.is_some() && evidence.fresh(image) {
            vec![image]
        } else {
            evidence.runtime_images(image)
        };
        runtime.retain(|obs|matches!(&obs.data,Data::Image{desired,..}if images::canonical(desired).as_deref()==Some(canonical.as_str())));
        let digests: BTreeSet<_> = runtime
            .iter()
            .filter_map(|obs| match &obs.data {
                Data::Image {
                    observed_digest: Some(digest),
                    ..
                } => images::digest(digest),
                _ => None,
            })
            .collect();
        let registries: Vec<_> = evidence
            .artifacts
            .get(canonical.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .copied()
            .filter(
                |obs| matches!(&obs.data,Data::Artifact{image,built:false,..}if image==&canonical),
            )
            .collect();
        let expected: BTreeSet<_> = if let Some(digest) = desired
            .split_once('@')
            .and_then(|(_, digest)| images::digest(digest))
        {
            BTreeSet::from([digest])
        } else {
            registries
                .iter()
                .filter_map(|obs| match &obs.data {
                    Data::Artifact { digest, tags, .. }
                        if images::tag(desired)
                            .is_some_and(|tag| tags.iter().any(|candidate| candidate == tag)) =>
                    {
                        Some(digest.as_str())
                    }
                    _ => None,
                })
                .collect()
        };
        let wanted = if expected.len() == 1 {
            expected.first().copied()
        } else {
            None
        };
        let pending=evidence.grace(image,job,now) || registries.iter().any(|obs|matches!(&obs.data,Data::Artifact{digest,created_at:Some(at),..}if Some(digest.as_str())==wanted && (now-*at).num_seconds()>=0 && (now-*at).num_seconds()<job.settings.rollout_grace.0 as i64));
        let children: BTreeSet<_> = registries
            .iter()
            .filter_map(|obs| match &obs.data {
                Data::Artifact {
                    digest,
                    children,
                    manifest: ManifestKind::Index,
                    ..
                } if Some(digest.as_str()) == wanted => Some(children),
                _ => None,
            })
            .flatten()
            .map(String::as_str)
            .collect();
        let comparable=registries.iter().any(|obs|matches!(&obs.data,Data::Artifact{digest,manifest:ManifestKind::Image,..}if Some(digest.as_str())==wanted)) || !children.is_empty();
        let mut mismatch = !pending
            && comparable
            && !digests.is_empty()
            && wanted.is_some_and(|wanted| {
                digests
                    .iter()
                    .any(|digest| *digest != wanted && !children.contains(digest))
            });
        let registry_verified = !digests.is_empty()
            && wanted.is_some_and(|wanted| {
                digests
                    .iter()
                    .all(|digest| *digest == wanted || children.contains(digest))
            })
            && digests.iter().all(|digest| {
                registries
                    .iter()
                    .any(|obs| matches!(&obs.data,Data::Artifact{digest:known,..}if known==digest))
                    || children.contains(digest)
            });
        let builds:Vec<_>=evidence.artifacts.get(canonical.as_str()).map(Vec::as_slice).unwrap_or(&[]).iter().copied().filter(|obs|matches!(&obs.data,Data::Artifact{digest,built:true,..}if digests.contains(digest.as_str()) || Some(digest.as_str())==wanted && !children.is_empty())).collect();
        let mut revisions: BTreeSet<String> = builds
            .iter()
            .filter_map(|obs| match &obs.data {
                Data::Artifact {
                    revision: Some(revision),
                    ..
                } => Some(revision.clone()),
                _ => None,
            })
            .collect();
        let mapped:Vec<_>=evidence.facts.iter().copied().filter(|obs|obs.resource.starts_with(&format!("{}/",job.target.name)) && matches!(&obs.data,Data::Build{target,revision:built_revision,state:ServiceState::Ready,..}if !target.is_empty() && canonical.ends_with(&format!("/{target}")) && revision.as_ref().is_some_and(|revision|images::revision_matches(revision,built_revision)))).collect();
        revisions.extend(mapped.iter().filter_map(|obs| match &obs.data {
            Data::Build { revision, .. } => Some(revision.clone()),
            _ => None,
        }));
        let revision = if revisions.len() == 1 {
            revisions.first().cloned()
        } else {
            None
        };
        if !pending
            && let Data::Image {
                revision: Some(wanted),
                ..
            } = &image.data
            && revision
                .as_ref()
                .is_some_and(|actual| !images::revision_matches(wanted, actual))
        {
            mismatch = true;
        }
        let build_verified = revision.is_some()
            && !digests.is_empty()
            && (digests.iter().all(|digest| {
                builds
                    .iter()
                    .any(|obs| matches!(&obs.data,Data::Artifact{digest:known,..}if known==digest || Some(known.as_str())==wanted && children.contains(digest)))
            }) || !mapped.is_empty());
        let repositories: BTreeSet<_> = builds
            .iter()
            .filter_map(|obs| match &obs.data {
                Data::Artifact {
                    repository: Some(repository),
                    ..
                } => Some(repository.as_str()),
                _ => None,
            })
            .chain(mapped.iter().filter_map(|obs| {
                match &obs.data {
                    Data::Build { pipeline, .. } => job
                        .target
                        .build_repositories
                        .get(pipeline)
                        .map(String::as_str),
                    _ => None,
                }
            }))
            .collect();
        let commits:Vec<_>=revision.as_ref().and_then(|revision|evidence.commits.get(revision.as_str())).map(Vec::as_slice).unwrap_or(&[]).iter().copied().filter(|obs|matches!(&obs.data,Data::Commit{repository,..}if repositories.is_empty() || repositories.contains(repository.as_str()))).collect();
        let commit_repositories: BTreeSet<_> = commits
            .iter()
            .filter_map(|obs| match &obs.data {
                Data::Commit { repository, .. } => Some(repository.clone()),
                _ => None,
            })
            .collect();
        let commit_verified = commit_repositories.len() == 1;
        let repository = if commit_verified {
            commit_repositories.first().cloned()
        } else {
            None
        };
        let at = runtime
            .iter()
            .chain(registries.iter())
            .chain(builds.iter())
            .chain(mapped.iter())
            .chain(commits.iter())
            .map(|obs| obs.observed_at)
            .max()
            .unwrap_or(image.observed_at);
        let valid_until = runtime
            .iter()
            .chain(registries.iter())
            .chain(builds.iter())
            .chain(mapped.iter())
            .chain(commits.iter())
            .map(|obs| evidence.expires(obs))
            .min()
            .unwrap_or_else(|| evidence.expires(image));
        let coverage = if digests.len() > 16 {
            Coverage::Truncated
        } else if inactive || pending || registry_verified && build_verified && commit_verified {
            Coverage::Complete
        } else {
            Coverage::Missing
        };
        operations.push(operation(id.clone(), coverage, at, true));
        observations.push(Observation {
            resource: format!("{}/provenance", image.resource),
            operation: id,
            observed_at: at,
            expected: if inactive {
                Expected::ScaleToZero
            } else {
                image.expected
            },
            data: Data::Provenance {
                valid_until,
                observed_digests: digests.into_iter().take(16).map(String::from).collect(),
                desired_digest: wanted.map(String::from),
                pending,
                revision,
                repository,
                registry_verified,
                build_verified,
                commit_verified,
                mismatch,
            },
        });
    }
    drop(evidence);
    current.operations.extend(operations);
    current.observations.extend(observations);
}
fn operation(id: String, coverage: Coverage, at: DateTime<Utc>, required: bool) -> Operation {
    Operation {
        id,
        coverage,
        observed_at: at,
        records: 1,
        pages: 0,
        attempts: 0,
        required,
    }
}

/// Evaluate conformance without turning deployment drift into an availability outage.
pub fn evaluate(obs: &Observation, now: DateTime<Utc>) -> crate::policy::Evaluation {
    use crate::policy::{fault, result};
    let inactive = obs.expected != Expected::Active;
    match &obs.data {
        Data::Provenance { valid_until, .. } if *valid_until < now => result(Health::Unknown),
        Data::Provenance { .. } if inactive => result(Health::ExpectedInactive),
        Data::Provenance { mismatch: true, .. } => fault(
            obs,
            "release-revision-mismatch",
            Severity::Warning,
            Confidence::Correlated,
        ),
        Data::Provenance {
            registry_verified: true,
            build_verified: true,
            commit_verified: true,
            ..
        } => result(Health::Healthy),
        Data::Provenance { .. } => result(Health::Unknown),
        _ => result(Health::Unknown),
    }
}

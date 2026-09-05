//! Provenance requires independent registry, build, and repository evidence.
pub mod support;
use chrono::Duration;
use monitor_core::{model::*, releases};
use support::release::{fixture, trace};
#[test]
fn complete_chain_verifies_the_runtime_image() -> Result<(), Box<dyn std::error::Error>> {
    let (job, snapshot, mut raw, now) = fixture()?;
    releases::enrich(&snapshot, &job, &mut raw, now);
    assert!(raw.complete());
    assert!(trace(&raw).is_some_and(|obs| matches!(
        obs.data,
        Data::Provenance {
            registry_verified: true,
            build_verified: true,
            commit_verified: true,
            mismatch: false,
            ..
        }
    )));
    Ok(())
}
#[test]
fn a_registry_tag_alone_cannot_establish_build_or_commit_provenance()
-> Result<(), Box<dyn std::error::Error>> {
    let (job, mut snapshot, mut raw, now) = fixture()?;
    snapshot.results.remove("source/Github");
    raw.observations
        .retain(|obs| !matches!(obs.data, Data::Artifact { built: true, .. }));
    releases::enrich(&snapshot, &job, &mut raw, now);
    assert!(!raw.complete());
    assert!(trace(&raw).is_some_and(|obs| matches!(
        obs.data,
        Data::Provenance {
            registry_verified: true,
            build_verified: false,
            commit_verified: false,
            ..
        }
    )));
    Ok(())
}
#[test]
fn source_revision_mismatch_is_not_a_running_service_outage()
-> Result<(), Box<dyn std::error::Error>> {
    let (job, mut snapshot, mut raw, now) = fixture()?;
    if let Data::Artifact { revision, .. } = &mut raw.observations[1].data {
        *revision = Some("c".repeat(40));
    }
    if let Some(result) = snapshot.results.get_mut("source/Github")
        && let Data::Commit { revision, .. } = &mut result.observations[0].data
    {
        *revision = "c".repeat(40);
    }
    releases::enrich(&snapshot, &job, &mut raw, now);
    let evaluation =
        monitor_core::policy::evaluate(trace(&raw).ok_or("trace")?, None, &job.settings, now);
    assert_eq!(evaluation.health, Health::Degraded);
    assert_eq!(evaluation.findings[0].severity, Severity::Warning);
    Ok(())
}
#[test]
fn a_different_repository_cannot_supply_the_commit_link() -> Result<(), Box<dyn std::error::Error>>
{
    let (job, mut snapshot, mut raw, now) = fixture()?;
    if let Some(result) = snapshot.results.get_mut("source/Github")
        && let Data::Commit { repository, .. } = &mut result.observations[0].data
    {
        *repository = "unrelated/repo".into();
    }
    releases::enrich(&snapshot, &job, &mut raw, now);
    assert!(!raw.complete());
    Ok(())
}
#[test]
fn known_index_children_verify_without_false_digest_mismatches()
-> Result<(), Box<dyn std::error::Error>> {
    let (job, mut snapshot, mut raw, now) = fixture()?;
    let child = format!("sha256:{}", "d".repeat(64));
    if let Data::Artifact {
        manifest, children, ..
    } = &mut raw.observations[0].data
    {
        *manifest = ManifestKind::Index;
        children.push(child.clone());
    }
    if let Some(result) = snapshot.results.get_mut("dev/Kubernetes")
        && let Data::Image {
            observed_digest, ..
        } = &mut result.observations[0].data
    {
        *observed_digest = Some(child);
    }
    releases::enrich(&snapshot, &job, &mut raw, now);
    assert!(raw.complete());
    assert!(trace(&raw).is_some_and(|obs| matches!(
        obs.data,
        Data::Provenance {
            mismatch: false,
            build_verified: true,
            ..
        }
    )));
    Ok(())
}
#[test]
fn missing_index_relationships_are_unknown_instead_of_false_drift()
-> Result<(), Box<dyn std::error::Error>> {
    let (job, mut snapshot, mut raw, now) = fixture()?;
    if let Data::Artifact { manifest, .. } = &mut raw.observations[0].data {
        *manifest = ManifestKind::Index;
    }
    if let Some(result) = snapshot.results.get_mut("dev/Kubernetes")
        && let Data::Image {
            observed_digest, ..
        } = &mut result.observations[0].data
    {
        *observed_digest = Some(format!("sha256:{}", "d".repeat(64)));
    }
    releases::enrich(&snapshot, &job, &mut raw, now);
    assert!(!raw.complete());
    assert!(trace(&raw).is_some_and(|obs| matches!(
        obs.data,
        Data::Provenance {
            mismatch: false,
            ..
        }
    )));
    Ok(())
}
#[test]
fn failed_workload_refresh_cannot_reuse_an_older_complete_copy()
-> Result<(), Box<dyn std::error::Error>> {
    let (job, mut snapshot, mut raw, now) = fixture()?;
    let old = snapshot
        .results
        .get("dev/Kubernetes")
        .ok_or("kube")?
        .clone();
    snapshot.results.insert("dev/Inventory".into(), old);
    if let Some(result) = snapshot.results.get_mut("dev/Kubernetes") {
        result.operations[0].coverage = Coverage::Denied;
        result.operations[0].observed_at = now + Duration::seconds(1);
    }
    releases::enrich(&snapshot, &job, &mut raw, now + Duration::seconds(1));
    assert!(!raw.complete());
    assert!(trace(&raw).is_some_and(|obs| matches!(
        obs.data,
        Data::Provenance {
            registry_verified: false,
            ..
        }
    )));
    Ok(())
}
#[test]
fn registry_freshness_cannot_extend_workload_evidence_expiry()
-> Result<(), Box<dyn std::error::Error>> {
    let (job, mut snapshot, mut raw, now) = fixture()?;
    snapshot.freshness.insert("dev/Kubernetes".into(), 60);
    releases::enrich(&snapshot, &job, &mut raw, now);
    let obs = trace(&raw).ok_or("trace")?;
    assert_eq!(
        monitor_core::policy::evaluate(obs, None, &job.settings, now + Duration::seconds(61))
            .health,
        Health::Unknown
    );
    Ok(())
}

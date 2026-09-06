//! Regression coverage for diagnostic retention, source grouping, and drill-down pagination.
use crate::{api::router, build_view::build, config::Config, test_support::*};
use axum::body::to_bytes;
use monitor_core::model::*;
use monitor_runtime::Observer;
use tower::ServiceExt;

#[test]
fn detection_times_survive_failed_reads_restart_and_recovery_confirmation()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut state, effective) = evidence()?;
    let job = effective
        .jobs
        .iter()
        .find(|job| job.check == Check::Edge)
        .ok_or("job")?;
    let first = state
        .snapshot
        .findings
        .values()
        .next()
        .ok_or("finding")?
        .diagnostic
        .clone()
        .ok_or("diagnostic")?;
    let mut result = state
        .snapshot
        .results
        .get(&job.key)
        .ok_or("result")?
        .clone();
    let at = first.last_detected_at + chrono::Duration::seconds(5);
    result.observations[0].observed_at = at;
    result.operations[0].observed_at = at;
    result.finished_at = at;
    state.apply(job, result.clone(), at);
    let bytes = serde_json::to_vec(&state.snapshot)?;
    state.snapshot = serde_json::from_slice(&bytes)?;
    let diagnostic = state
        .snapshot
        .findings
        .values()
        .next()
        .and_then(|f| f.diagnostic.as_ref())
        .ok_or("retained")?;
    assert_eq!(diagnostic.first_detected_at, first.first_detected_at);
    assert_eq!(diagnostic.last_detected_at, at);
    state.apply(
        job,
        CheckResult::failure(
            job.target.name.clone(),
            job.check,
            job.revision.clone(),
            Coverage::Denied,
        ),
        at,
    );
    assert_eq!(
        state
            .snapshot
            .findings
            .values()
            .next()
            .and_then(|f| f.diagnostic.as_ref())
            .ok_or("failure")?
            .last_detected_at,
        at
    );
    if let Data::Endpoint { status, .. } = &mut result.observations[0].data {
        *status = Some(200);
    }
    result.observations[0].observed_at = at + chrono::Duration::seconds(5);
    result.operations[0].observed_at = result.observations[0].observed_at;
    state.apply(job, result, at + chrono::Duration::seconds(5));
    assert_eq!(
        state
            .snapshot
            .findings
            .values()
            .next()
            .and_then(|f| f.diagnostic.as_ref())
            .ok_or("pending recovery")?
            .last_detected_at,
        at
    );
    Ok(())
}

#[test]
fn all_sources_survive_newer_metadata_and_resource_clones_share_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut state, effective) = evidence()?;
    let result = state.snapshot.results.values_mut().next().ok_or("result")?;
    let mut metadata = result.observations[0].clone();
    metadata.observed_at += chrono::Duration::seconds(1);
    metadata.data = Data::Inventory {
        family: "endpoint".into(),
        supported: true,
    };
    result.observations.push(metadata);
    let view = build(&state.snapshot, &effective, 1);
    let resource = view.resources.first().ok_or("resource")?;
    assert_eq!(resource.evidence.len(), 2);
    assert!(
        resource
            .evidence
            .iter()
            .any(|e| e.facts.iter().any(|f| f.value == "503"))
    );
    assert!(std::sync::Arc::ptr_eq(
        &resource.evidence,
        &resource.clone().evidence
    ));
    Ok(())
}

#[tokio::test]
async fn operations_are_searchable_and_paginated_beyond_overview_preview()
-> Result<(), Box<dyn std::error::Error>> {
    let config = Config::default();
    let (app, _) = app(&config).await?;
    let (mut state, effective) = evidence()?;
    let result = state.snapshot.results.values_mut().next().ok_or("result")?;
    let mut operation = result.operations[0].clone();
    operation.coverage = Coverage::Truncated;
    result.operations = (0..201)
        .map(|i| {
            let mut op = operation.clone();
            op.id = format!("operation-{i:04}");
            op
        })
        .collect();
    app.bus.update(&state.snapshot, &effective, &[]);
    let routes = router(app, &config);
    for (path, count, total) in [
        (
            "/api/v1/check/operations?target=fixture&check=edge&limit=100",
            100,
            201,
        ),
        (
            "/api/v1/check/operations?target=fixture&check=edge&q=0200",
            1,
            1,
        ),
        ("/api/v1/resource/evidence?id=fixture/endpoints/api", 1, 1),
    ] {
        let response = routes.clone().oneshot(request(path)?).await?;
        assert_eq!(response.status(), 200);
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await?)?;
        assert_eq!(body["items"].as_array().ok_or("items")?.len(), count);
        assert_eq!(body["total"], total);
    }
    let response = routes
        .oneshot(request("/api/v1/check?target=fixture&check=edge")?)
        .await?;
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await?)?;
    assert_eq!(body["required_failures"], 201);
    assert_eq!(body["failures"].as_array().ok_or("failures")?.len(), 3);
    Ok(())
}

#[test]
fn legacy_findings_keep_unknown_start_and_replaced_pods_have_no_restart_delta()
-> Result<(), Box<dyn std::error::Error>> {
    let (state, _) = evidence()?;
    let mut finding = state
        .snapshot
        .findings
        .values()
        .next()
        .ok_or("finding")?
        .clone();
    let mut value = serde_json::to_value(&finding)?;
    value.as_object_mut().ok_or("object")?.remove("diagnostic");
    finding = serde_json::from_value(value)?;
    let mut observation = state
        .snapshot
        .results
        .values()
        .next()
        .ok_or("result")?
        .observations[0]
        .clone();
    observation.data = Data::Pod {
        uid: "old".into(),
        container: "worker".into(),
        ready: false,
        restarts: 20,
        crash_loop: true,
        created_at: None,
        terminated_at: None,
    };
    let previous = observation.clone();
    if let Data::Pod { uid, .. } = &mut observation.data {
        *uid = "replacement".into();
    }
    let diagnostic = monitor_core::diagnostics::Diagnostic::capture(
        &observation,
        Some(&previous),
        Some(&finding),
        600,
    );
    assert!(diagnostic.first_detected_at.is_none());
    assert!(diagnostic.previous_restarts.is_none());
    Ok(())
}

#[test]
fn retained_findings_remain_navigable_without_current_inventory()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut state, effective) = evidence()?;
    state.snapshot.results.clear();
    let view = build(&state.snapshot, &effective, 1);
    assert_eq!(view.resources.len(), 1);
    assert_eq!(view.resources[0].health, Health::Unknown);
    assert!(
        view.resources[0]
            .facts
            .iter()
            .any(|fact| fact.value == "503")
    );
    Ok(())
}

#[test]
fn release_mismatch_evidence_includes_the_actual_and_expected_digests() {
    let data = Data::Provenance {
        valid_until: chrono::Utc::now(),
        observed_digests: vec!["sha256:observed".into()],
        desired_digest: Some("sha256:desired".into()),
        pending: false,
        revision: Some("revision".into()),
        repository: Some("example/service".into()),
        registry_verified: false,
        build_verified: true,
        commit_verified: true,
        mismatch: true,
    };
    let facts = crate::facts::facts(&data);
    assert!(
        facts
            .iter()
            .any(|fact| fact.label == "Desired image digest" && fact.value == "sha256:desired")
    );
    assert!(
        facts
            .iter()
            .any(|fact| fact.label == "Observed image digests" && fact.value == "sha256:observed")
    );
}

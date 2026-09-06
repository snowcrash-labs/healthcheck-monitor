//! View limits, source freshness, and coalesced delivery preserve collection semantics.
use crate::{api::router, build_view::build, config::Config, test_support::*};
use axum::body::to_bytes;
use futures::StreamExt;
use monitor_core::model::*;
use monitor_runtime::Observer;
use tower::ServiceExt;

#[test]
fn all_collected_failure_details_remain_available() -> Result<(), Box<dyn std::error::Error>> {
    let (mut state, effective) = evidence()?;
    let job = effective
        .jobs
        .iter()
        .find(|job| job.check == Check::Edge)
        .ok_or("edge")?;
    let result = state.snapshot.results.get_mut(&job.key).ok_or("result")?;
    let mut failure = result.operations[0].clone();
    failure.coverage = Coverage::Denied;
    result.operations = vec![failure; 2000];
    let view = build(&state.snapshot, &effective, 1);
    assert_eq!(
        view.checks
            .iter()
            .find(|check| check.check == Check::Edge)
            .ok_or("check")?
            .failures
            .len(),
        2000
    );
    assert_eq!(view.resources.len(), 1);
    Ok(())
}

#[test]
fn shared_resources_prefer_the_newest_source_and_preserve_expiry()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut state, effective) = evidence()?;
    let edge = effective
        .jobs
        .iter()
        .find(|job| job.check == Check::Edge)
        .ok_or("edge")?;
    let other = effective
        .jobs
        .iter()
        .find(|job| job.check != Check::Edge)
        .ok_or("other")?;
    let mut result = state
        .snapshot
        .results
        .get(&edge.key)
        .ok_or("result")?
        .clone();
    result.check = other.check;
    result.observations[0].observed_at -= chrono::Duration::minutes(10);
    state.snapshot.results.insert(other.key.clone(), result);
    let view = build(&state.snapshot, &effective, 1);
    assert_eq!(view.resources.len(), 1);
    assert_eq!(view.resources[0].check, Check::Edge);
    assert_eq!(
        crate::view::current_health(
            Health::Healthy,
            view.resources[0].expires_at,
            view.resources[0].expires_at + chrono::Duration::seconds(1)
        ),
        Health::Unknown
    );
    Ok(())
}

#[tokio::test]
async fn complete_collection_does_not_establish_health() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::default();
    let (app, _) = app(&config).await?;
    let (mut state, effective) = evidence()?;
    state.snapshot.findings.clear();
    for value in state.snapshot.health.values_mut() {
        *value = Health::Unknown;
    }
    for job in &effective.jobs {
        state
            .snapshot
            .results
            .entry(job.key.clone())
            .or_insert_with(|| {
                CheckResult::failure(
                    job.target.name.clone(),
                    job.check,
                    effective.revision.clone(),
                    Coverage::Complete,
                )
            });
    }
    app.bus.update(&state.snapshot, &effective, &[]);
    let response = router(app, &config)
        .oneshot(request("/api/v1/overview")?)
        .await?;
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await?)?;
    assert_eq!(value["totals"]["incomplete_checks"], 0);
    assert_eq!(value["targets"][0]["health"], "unknown");
    Ok(())
}

#[tokio::test]
async fn event_streams_coalesce_updates_and_reconnect_at_current_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let config = Config::default();
    let (app, _) = app(&config).await?;
    let routes = router(app.clone(), &config);
    let response = routes.clone().oneshot(request("/api/v1/events")?).await?;
    let mut stream = response.into_body().into_data_stream();
    let first = stream.next().await.ok_or("initial announcement")??;
    assert!(std::str::from_utf8(&first)?.contains("\"generation\":1"));
    let (mut state, effective) = evidence()?;
    for _ in 0..3 {
        state.snapshot.captured_at += chrono::Duration::seconds(1);
        app.bus.update(&state.snapshot, &effective, &[]);
    }
    let latest = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
        .await?
        .ok_or("latest announcement")??;
    assert!(std::str::from_utf8(&latest)?.contains("\"generation\":4"));
    drop(stream);
    let response = routes.oneshot(request("/api/v1/events")?).await?;
    let first = response
        .into_body()
        .into_data_stream()
        .next()
        .await
        .ok_or("reconnected announcement")??;
    assert!(std::str::from_utf8(&first)?.contains("\"generation\":4"));
    app.stop.cancel();
    Ok(())
}

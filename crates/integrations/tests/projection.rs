//! Metadata projection, desired-state parsing, and redaction regressions.
use monitor_core::{
    config::{resolve::Selection, types::Config},
    model::*,
};
use monitor_integrations::{
    github::build_targets,
    kube_projection,
    logs::{Groups, classify},
    nats::parse_report,
};
use serde_json::json;
fn job() -> Result<monitor_core::config::resolve::Job, Box<dyn std::error::Error>> {
    Config::parse("version=1\n[[targets]]\nname='dev'\nprovider='kubernetes'\nscope='dev'")?
        .resolve(&Selection::default())?
        .jobs
        .into_iter()
        .next()
        .ok_or_else(|| "missing job".into())
}
#[test]
fn pod_hostname_is_not_public_routing() -> Result<(), Box<dyn std::error::Error>> {
    let value = json!({"metadata":{"name":"nats-0","namespace":"transcription"},"spec":{"hostname":"nats-0"},"status":{"phase":"Running","containerStatuses":[]}});
    let observations = kube_projection::project(&job()?, "pods", &value);
    assert!(
        !observations
            .iter()
            .any(|o| matches!(o.data, Data::AdvertisedEndpoint { .. }))
    );
    Ok(())
}
#[test]
fn warning_events_are_diagnostic_samples() -> Result<(), Box<dyn std::error::Error>> {
    let value = json!({"metadata":{"name":"autoscaling","namespace":"x"},"type":"Warning","reason":"FailedScheduling","lastTimestamp":chrono::Utc::now().to_rfc3339(),"count":2});
    let observations = kube_projection::project(&job()?, "events", &value);
    assert!(!observations.iter().any(|o| matches!(
        o.data,
        Data::Condition {
            healthy: Some(false),
            ..
        }
    )));
    Ok(())
}
#[test]
fn desired_build_targets_are_unique_sorted_and_structural() -> Result<(), Box<dyn std::error::Error>>
{
    assert_eq!(
        build_targets(
            "steps:\n - {id: build-zeta}\n - id: build-alpha\n - id: build-zeta\n - id: deploy-alpha\n"
        )?,
        vec!["alpha", "zeta"]
    );
    assert!(build_targets("steps: []").is_err());
    assert!(build_targets("comment: '- id: build-not-a-step'").is_err());
    Ok(())
}
#[test]
fn immutable_digest_and_commit_remain_separate() {
    let (image, digest, revision) = kube_projection::image_parts(
        "us-central1-docker.pkg.dev/ops/apps/api:release-ce53323",
        Some("us-central1-docker.pkg.dev/ops/apps/api@sha256:abc123"),
    );
    assert!(image.ends_with(":release-ce53323"));
    assert_eq!(digest.as_deref(), Some("sha256:abc123"));
    assert_eq!(revision.as_deref(), Some("ce53323"));
}
#[test]
fn cronjob_template_has_image_without_live_pod() -> Result<(), Box<dyn std::error::Error>> {
    let value = json!({"metadata":{"name":"cleanup","namespace":"jobs"},"spec":{"suspend":true,"jobTemplate":{"spec":{"template":{"spec":{"containers":[{"name":"cleanup","image":"registry/project/cleanup:sha-abcdef1","env":[{"name":"SECRET","value":"private-value"}]}]}}}}}});
    let observations = kube_projection::project(&job()?, "cronjobs", &value);
    assert!(
        observations
            .iter()
            .any(|o| matches!(&o.data,Data::Image{revision:Some(r),..}if r=="abcdef1"))
    );
    assert!(!serde_json::to_string(&observations)?.contains("private-value"));
    Ok(())
}
#[test]
fn nats_aggregates_are_numeric_and_bad_headers_fail() {
    let rows = parse_report(
        "│ Stream │ Storage │ Consumers │ Messages │ Bytes │ Lost │\n│ SONGS │ File │ 2 │ 17 │ 2048 │ 0 │",
        10,
    );
    assert!(matches!(rows,Ok(ref rows)if rows==&vec![("SONGS".into(),17,2048)]));
    assert!(parse_report("permission denied", 10).is_err());
}
#[test]
fn log_grouping_never_retains_customer_or_credential_values()
-> Result<(), Box<dyn std::error::Error>> {
    let mut groups = Groups::default();
    groups.add(
        "task 123 failed for customer alice@example.com Bearer abcdefghijklmnopqrstuvwxyz",
        chrono::Utc::now(),
    );
    groups.add(
        "task 456 failed for customer bob@example.com",
        chrono::Utc::now(),
    );
    let values = groups.finish();
    assert_eq!(values.len(), 1);
    let encoded = serde_json::to_string(&values)?;
    for secret in [
        "alice",
        "bob",
        "Bearer",
        "abcdefghijklmnopqrstuvwxyz",
        "123",
        "456",
    ] {
        assert!(!encoded.contains(secret));
    }
    Ok(())
}
#[test]
fn runtime_warnings_remain_classified_but_not_error_signals() {
    assert_eq!(
        classify("module.py:12: UserWarning: backend is deprecated"),
        LogClass::Warning
    );
    assert_eq!(
        classify("support.py:9: FutureWarning: runtime is old"),
        LogClass::Warning
    );
    assert_eq!(classify("database unavailable"), LogClass::Connection);
}
#[test]
fn keda_projection_excludes_auth_and_environment_metadata() -> Result<(), Box<dyn std::error::Error>>
{
    let value = json!({"metadata":{"name":"worker","namespace":"jobs"},"spec":{"scaleTargetRef":{"name":"worker"},"triggers":[{"metadata":{"activationLagCount":"500","addressFromEnv":"REDIS_PASSWORD","password":"customer-secret"}}]},"status":{"externalMetricNames":["s0-redis-streams"],"conditions":[{"type":"Ready","status":"True"}]}});
    let observations = kube_projection::project(&job()?, "scaledobjects", &value);
    assert!(observations.iter().any(|o| matches!(
        o.data,
        Data::Scaler {
            activation: 500.0,
            ready: true,
            ..
        }
    )));
    let encoded = serde_json::to_string(&observations)?;
    assert!(!encoded.contains("REDIS_PASSWORD"));
    assert!(!encoded.contains("customer-secret"));
    Ok(())
}
#[test]
fn external_secret_sync_age_and_nonperiodic_modes_are_distinct()
-> Result<(), Box<dyn std::error::Error>> {
    let now = chrono::Utc::now();
    let mut value = json!({"metadata":{"name":"credential","namespace":"jobs"},"spec":{"refreshInterval":"1h"},"status":{"refreshTime":(now-chrono::Duration::hours(3)).to_rfc3339(),"conditions":[{"type":"Ready","status":"True"}]}});
    let settings = monitor_core::config::settings::Settings::default();
    let observations = kube_projection::project(&job()?, "externalsecrets", &value);
    let observation = observations
        .iter()
        .find(|observation| matches!(observation.data, Data::Synchronization { .. }))
        .ok_or("missing synchronization")?;
    assert_eq!(
        monitor_core::policy::evaluate(observation, None, &settings, now).health,
        Health::Degraded
    );
    for interval in ["0", "0s", "0m"] {
        value["spec"]["refreshInterval"] = json!(interval);
        let observations = kube_projection::project(&job()?, "externalsecrets", &value);
        assert!(observations.iter().any(|observation| matches!(
            observation.data,
            Data::Synchronization {
                interval_seconds: None,
                ready: Some(true),
                ..
            }
        )));
    }
    value["spec"]["refreshInterval"] = json!("1h");
    value["spec"]["refreshPolicy"] = json!("OnChange");
    let observations = kube_projection::project(&job()?, "externalsecrets", &value);
    assert!(observations.iter().any(|observation| matches!(
        observation.data,
        Data::Synchronization {
            interval_seconds: None,
            ..
        }
    )));
    Ok(())
}
#[test]
fn ready_nodes_with_pressure_are_unhealthy_while_draining_nodes_keep_their_grace()
-> Result<(), Box<dyn std::error::Error>> {
    let job = job()?;
    let mut value = json!({"metadata":{"name":"node","creationTimestamp":"2020-01-01T00:00:00Z"},"status":{"conditions":[{"type":"Ready","status":"True"},{"type":"DiskPressure","status":"True"}]}});
    let observations = kube_projection::project(&job, "nodes", &value);
    let observation = observations
        .iter()
        .find(|observation| matches!(observation.data, Data::Workload { node: true, .. }))
        .ok_or("missing node")?;
    assert_eq!(
        monitor_core::policy::evaluate(observation, None, &job.settings, chrono::Utc::now()).health,
        Health::Unhealthy
    );
    value["spec"] =
        json!({"taints":[{"key":"ToBeDeletedByClusterAutoscaler","effect":"NoSchedule"}]});
    let observations = kube_projection::project(&job, "nodes", &value);
    let observation = observations
        .iter()
        .find(|observation| matches!(observation.data, Data::Workload { node: true, .. }))
        .ok_or("missing node")?;
    assert_eq!(
        monitor_core::policy::evaluate(observation, None, &job.settings, chrono::Utc::now()).health,
        Health::ExpectedInactive
    );
    Ok(())
}

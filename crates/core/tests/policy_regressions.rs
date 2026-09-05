//! Regressions for strict endpoints and terminal Job ordering.
use chrono::Utc;
use monitor_core::{config::settings::Settings, model::*, policy::evaluate};
fn observation(data: Data) -> Observation {
    Observation {
        resource: "test/resource".into(),
        operation: "test".into(),
        observed_at: Utc::now(),
        expected: Expected::Active,
        data,
    }
}
#[test]
fn configured_endpoint_rejects_root_reachability_status() {
    let obs = observation(Data::Endpoint {
        dns: true,
        tls: true,
        status: Some(404),
        accepted: vec![200],
        latency_ms: 3,
        expires_at: None,
    });
    assert_eq!(
        evaluate(&obs, None, &Settings::default(), Utc::now()).health,
        Health::Unhealthy
    );
}
#[test]
fn endpoint_dns_tls_and_connection_failures_reach_policy() {
    for (dns, tls, status) in [
        (false, true, Some(200)),
        (true, false, Some(200)),
        (true, true, None),
        (true, true, Some(503)),
    ] {
        let obs = observation(Data::Endpoint {
            dns,
            tls,
            status,
            accepted: vec![200],
            latency_ms: 3,
            expires_at: None,
        });
        assert!(
            !evaluate(&obs, None, &Settings::default(), Utc::now())
                .findings
                .is_empty()
        );
    }
}
#[test]
fn successful_job_with_failed_attempts_is_recovered() {
    let obs = observation(Data::Job {
        completed_at: None,
        scheduled_at: None,
        complete: true,
        failed: false,
        failed_attempts: 2,
        succeeded: 1,
        active: 0,
        created_at: None,
    });
    assert_eq!(
        evaluate(&obs, None, &Settings::default(), Utc::now()).health,
        Health::Healthy
    );
}
#[test]
fn partial_job_success_is_not_completion() {
    let obs = observation(Data::Job {
        completed_at: None,
        scheduled_at: None,
        complete: false,
        failed: false,
        failed_attempts: 1,
        succeeded: 1,
        active: 1,
        created_at: None,
    });
    assert_eq!(
        evaluate(&obs, None, &Settings::default(), Utc::now()).health,
        Health::Unknown
    );
}
#[test]
fn contradictory_job_conditions_are_invalid_evidence() {
    let obs = observation(Data::Job {
        completed_at: None,
        scheduled_at: None,
        complete: true,
        failed: true,
        failed_attempts: 1,
        succeeded: 1,
        active: 0,
        created_at: None,
    });
    assert_eq!(
        evaluate(&obs, None, &Settings::default(), Utc::now()).health,
        Health::Unknown
    );
}

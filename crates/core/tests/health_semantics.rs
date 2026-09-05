//! Synthetic compatibility cases independent of the legacy evidence schema.
use chrono::{Duration, Utc};
use monitor_core::{config::settings::Settings, model::*, policy::evaluate};
fn obs(data: Data) -> Observation {
    Observation {
        resource: "test/resource".into(),
        operation: "test".into(),
        observed_at: Utc::now(),
        expected: Expected::Active,
        data,
    }
}
fn health(data: Data) -> Health {
    evaluate(&obs(data), None, &Settings::default(), Utc::now()).health
}
#[test]
fn zero_replicas_are_expected_inactive() {
    assert_eq!(
        health(Data::Workload {
            desired: 0,
            ready: 0,
            created_at: None,
            draining: false,
            node: false
        }),
        Health::ExpectedInactive
    );
}
#[test]
fn missing_ready_replicas_are_unhealthy() {
    assert_eq!(
        health(Data::Workload {
            desired: 1,
            ready: 0,
            created_at: None,
            draining: false,
            node: false
        }),
        Health::Unhealthy
    );
}
#[test]
fn fresh_pending_pod_has_rollout_grace() {
    assert_eq!(
        health(Data::Workload {
            desired: 1,
            ready: 0,
            created_at: Some(Utc::now()),
            draining: false,
            node: false
        }),
        Health::Unknown
    );
}
#[test]
fn starting_nodes_have_separate_grace() {
    assert_eq!(
        health(Data::Workload {
            desired: 1,
            ready: 0,
            created_at: Some(Utc::now() - Duration::seconds(240)),
            draining: false,
            node: true
        }),
        Health::Unknown
    );
    assert_eq!(
        health(Data::Workload {
            desired: 1,
            ready: 0,
            created_at: Some(Utc::now() - Duration::seconds(360)),
            draining: false,
            node: true
        }),
        Health::Unhealthy
    );
}
#[test]
fn draining_nodes_are_expected() {
    assert_eq!(
        health(Data::Workload {
            desired: 1,
            ready: 0,
            created_at: None,
            draining: true,
            node: true
        }),
        Health::ExpectedInactive
    );
}
#[test]
fn historical_failed_job_does_not_dominate_current_health() {
    assert_ne!(
        health(Data::Job {
            complete: false,
            failed: true,
            failed_attempts: 1,
            succeeded: 0,
            active: 0,
            created_at: Some(Utc::now() - Duration::days(2))
        }),
        Health::Unhealthy
    );
}
#[test]
fn partial_success_without_terminal_condition_is_unknown() {
    assert_eq!(
        health(Data::Job {
            complete: false,
            failed: false,
            failed_attempts: 1,
            succeeded: 1,
            active: 0,
            created_at: None
        }),
        Health::Unknown
    );
}
fn pod(uid: &str, restarts: u32) -> Data {
    Data::Pod {
        uid: uid.into(),
        container: "worker".into(),
        ready: true,
        restarts,
        crash_loop: false,
        created_at: None,
        terminated_at: None,
    }
}
#[test]
fn old_restart_counter_does_not_imply_current_failure() {
    assert_eq!(health(pod("a", 99)), Health::Healthy);
}
#[test]
fn restart_delta_compares_same_pod_and_container() {
    let old = obs(pod("a", 1));
    let new = obs(pod("a", 5));
    assert_eq!(
        evaluate(&new, Some(&old), &Settings::default(), Utc::now()).health,
        Health::Degraded
    );
    let replacement = obs(pod("b", 5));
    assert_eq!(
        evaluate(&replacement, Some(&old), &Settings::default(), Utc::now()).health,
        Health::Healthy
    );
}
#[test]
fn cronjob_latest_schedule_needs_success() {
    assert_eq!(
        health(Data::Schedule {
            schedule: "0 * * * *".into(),
            timezone: "UTC".into(),
            suspended: false,
            active: 0,
            last_schedule: Some(Utc::now() - Duration::minutes(20)),
            last_success: Some(Utc::now() - Duration::days(1))
        }),
        Health::Degraded
    );
}
#[test]
fn suspended_cronjobs_are_expected_inactive() {
    assert_eq!(
        health(Data::Schedule {
            schedule: "0 * * * *".into(),
            timezone: "UTC".into(),
            suspended: true,
            active: 0,
            last_schedule: None,
            last_success: None
        }),
        Health::ExpectedInactive
    );
}
fn queue(backlog: f64, ready: u32, crash_loop: bool) -> Data {
    Data::Queue {
        backlog,
        activation: 0.0,
        ready,
        desired: ready.max(1),
        crash_loop,
        scaler_ready: true,
        age_seconds: None,
        dead_letters: None,
    }
}
#[test]
fn queue_crash_is_immediately_actionable() {
    assert_eq!(health(queue(2.0, 0, true)), Health::Unhealthy);
}
#[test]
fn queue_zero_with_scale_to_zero_is_healthy() {
    assert_eq!(health(queue(0.0, 0, false)), Health::Healthy);
}
#[test]
fn single_queue_observation_cannot_prove_persistence() {
    assert_eq!(health(queue(2.0, 0, false)), Health::Unknown);
}
#[test]
fn persistent_unready_worker_stays_warning_without_crash() {
    let old = obs(queue(2.0, 0, false));
    let new = obs(queue(2.0, 0, false));
    assert_eq!(
        evaluate(&new, Some(&old), &Settings::default(), Utc::now()).health,
        Health::Degraded
    );
}
#[test]
fn scale_up_can_recover_after_warning() {
    assert_eq!(health(queue(2.0, 1, false)), Health::Healthy);
}
#[test]
fn metric_requires_valid_capacity_or_configured_threshold() {
    assert_eq!(
        health(Data::Metric {
            name: "pressure".into(),
            value: 99.0,
            capacity: None,
            warning: None,
            error: None,
            window_seconds: 900
        }),
        Health::Unknown
    );
    assert_eq!(
        health(Data::Metric {
            name: "pressure".into(),
            value: 91.0,
            capacity: Some(100.0),
            warning: None,
            error: None,
            window_seconds: 900
        }),
        Health::Unhealthy
    );
}
#[test]
fn transient_threshold_does_not_prove_sustained_pressure() {
    assert_eq!(
        health(Data::Metric {
            name: "pressure".into(),
            value: 91.0,
            capacity: Some(100.0),
            warning: None,
            error: None,
            window_seconds: 60
        }),
        Health::Unknown
    );
}
#[test]
fn stale_success_cannot_establish_health() {
    let mut observation = obs(queue(0.0, 1, false));
    observation.observed_at = Utc::now() - Duration::days(1);
    assert_eq!(
        evaluate(&observation, None, &Settings::default(), Utc::now()).health,
        Health::Unknown
    );
}
#[test]
fn no_log_entries_cannot_establish_healthy_silence() {
    assert_eq!(
        health(Data::Log {
            signature: LogClass::OtherError,
            count: 0,
            first_seen: Utc::now(),
            last_seen: Utc::now(),
            sampled: true
        }),
        Health::Unknown
    );
}

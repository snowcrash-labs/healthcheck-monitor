//! Recovery-age policy uses the newest valid recovery point rather than every historical backup.
use chrono::{Duration, Utc};
use monitor_core::{
    config::{duration::Span, settings::Settings},
    model::*,
    policy::evaluate,
    recovery::consolidate,
};
#[test]
fn historical_points_do_not_override_the_latest_successful_backup() {
    let now = Utc::now();
    let point = |days| Observation {
        context: None,
        resource: "test/backups/database".into(),
        operation: "backups".into(),
        observed_at: now,
        expected: Expected::Active,
        data: Data::Recovery {
            last_attempt: Some(now - Duration::days(days)),
            state: ServiceState::Ready,
            enabled: Some(true),
            last_success: Some(now - Duration::days(days)),
            retention_days: Some(30),
            point_in_time: None,
            geo_redundant: None,
        },
    };
    let mut observations = vec![point(1), point(20), point(5)];
    consolidate(&mut observations);
    assert_eq!(observations.len(), 1);
    let settings = Settings {
        recovery_age_error: Some(Span(2 * 86400)),
        ..Default::default()
    };
    assert_eq!(
        evaluate(&observations[0], None, &settings, now).health,
        Health::Healthy
    );
    assert_eq!(
        evaluate(&observations[0], None, &Settings::default(), now).health,
        Health::Unknown
    );
}
#[test]
fn newest_failed_attempt_preserves_the_last_successful_recovery_point() {
    let now = Utc::now();
    let point = |state, at| Observation {
        context: None,
        resource: "test/backups/database".into(),
        operation: "backups".into(),
        observed_at: now,
        expected: Expected::Active,
        data: Data::Recovery {
            last_attempt: Some(at),
            state,
            enabled: Some(true),
            last_success: if state == ServiceState::Ready {
                Some(at)
            } else {
                None
            },
            retention_days: None,
            point_in_time: None,
            geo_redundant: None,
        },
    };
    let mut observations = vec![
        point(ServiceState::Ready, now - Duration::hours(1)),
        point(ServiceState::Failed, now),
    ];
    consolidate(&mut observations);
    assert!(
        matches!(observations[0].data,Data::Recovery{last_success:Some(at),..}if at==now-Duration::hours(1))
    );
    assert_eq!(
        evaluate(&observations[0], None, &Settings::default(), now).health,
        Health::Degraded
    );
}

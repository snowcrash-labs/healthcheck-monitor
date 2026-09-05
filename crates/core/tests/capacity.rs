//! Capacity persistence cannot be inferred from failed samples or a brief error-level spike.
use chrono::{DateTime, Duration, Utc};
use monitor_core::{
    capacity,
    config::{duration::Span, settings::Settings},
    model::*,
};
use std::collections::BTreeMap;
fn obs(at: DateTime<Utc>, value: f64) -> Observation {
    Observation {
        resource: "test/quota/cpu".into(),
        operation: "quota".into(),
        observed_at: at,
        expected: Expected::Active,
        data: Data::Metric {
            name: "cpu".into(),
            value,
            capacity: Some(100.0),
            warning: None,
            error: None,
            window_seconds: 0,
        },
    }
}
#[test]
fn warning_and_error_sustain_are_tracked_independently() {
    let now = Utc::now();
    let settings = Settings {
        interval: Span(30),
        ..Default::default()
    };
    let mut state = BTreeMap::new();
    for second in (0..=600).step_by(30) {
        let at = now + Duration::seconds(second);
        let value = if second == 600 { 95.0 } else { 85.0 };
        let evaluation = capacity::evaluate(&mut state, &obs(at, value), &settings, true, at);
        assert_eq!(
            evaluation.map(|evaluation| evaluation.health),
            Some(if second == 600 {
                Health::Degraded
            } else {
                Health::Unknown
            })
        );
    }
    for second in (630..=1200).step_by(30) {
        let at = now + Duration::seconds(second);
        let evaluation = capacity::evaluate(&mut state, &obs(at, 95.0), &settings, true, at);
        assert_eq!(
            evaluation.map(|evaluation| evaluation.health),
            Some(if second == 1200 {
                Health::Unhealthy
            } else {
                Health::Degraded
            })
        );
    }
}
#[test]
fn collection_failure_and_stale_gaps_reset_pressure_continuity() {
    let now = Utc::now();
    let settings = Settings::default();
    let mut state = BTreeMap::new();
    capacity::evaluate(&mut state, &obs(now, 95.0), &settings, true, now);
    let failed = now + Duration::seconds(300);
    capacity::evaluate(&mut state, &obs(failed, 95.0), &settings, false, failed);
    let next = now + Duration::seconds(600);
    let evaluation = capacity::evaluate(&mut state, &obs(next, 95.0), &settings, true, next);
    assert_eq!(
        evaluation.map(|evaluation| evaluation.health),
        Some(Health::Unknown)
    );
    let gap = next + Duration::seconds(1000);
    let evaluation = capacity::evaluate(&mut state, &obs(gap, 95.0), &settings, true, gap);
    assert_eq!(
        evaluation.map(|evaluation| evaluation.health),
        Some(Health::Unknown)
    );
}
#[test]
fn low_current_capacity_is_clear_evidence_without_waiting_for_another_sustain_window() {
    let now = Utc::now();
    let mut state = BTreeMap::new();
    let settings = Settings::default();
    let evaluation = capacity::evaluate(&mut state, &obs(now, 10.0), &settings, true, now);
    assert_eq!(
        evaluation.map(|evaluation| evaluation.health),
        Some(Health::Healthy)
    );
}

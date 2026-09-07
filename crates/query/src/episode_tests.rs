//! Episode boundaries prevent later recovery and reappearance from rewriting deployment comparisons.
use crate::{
    assessment::{Evidence, evaluate},
    enums::*,
    filter::{Deployment, Filter, Window},
    record::*,
    response::Availability,
};
use chrono::{Duration, Utc};
fn finding(
    at: chrono::DateTime<Utc>,
    first: Option<chrono::DateTime<Utc>>,
    closed: Option<chrono::DateTime<Utc>>,
) -> Record {
    Record {
        id: "episode".into(),
        identity: "api/pod-not-ready".into(),
        scope: Scope {
            target: "api".into(),
            provider: Provider::Gcp,
            scope: "project".into(),
        },
        location: Location::default(),
        check: Some(Check::Kubernetes),
        resource: Some("api/pod".into()),
        observed_at: at,
        last_observed_at: at,
        valid_until: Some(at + Duration::hours(1)),
        closed_at: closed,
        stale: false,
        details: Details::Finding {
            rule: "pod-not-ready".into(),
            severity: Severity::Error,
            state: if closed.is_some() {
                FindingState::Recovered
            } else {
                FindingState::Active
            },
            first_detected_at: first,
            expected: Expected::Active,
            confidence: Confidence::Direct,
            facts: vec![],
            links: vec![],
            legacy: first.is_none(),
        },
    }
}
#[test]
fn later_recovery_is_not_counted_inside_the_deployment_window() -> Result<(), crate::Error> {
    let deployed = Utc::now() - Duration::minutes(10);
    let q = Deployment {
        deployed_at: deployed,
        window_seconds: Some(60),
        expected_revision: None,
        expected_digest: None,
        filter: Filter {
            target: Some("api".into()),
            ..Default::default()
        },
    };
    let rows = [finding(
        deployed + Duration::seconds(10),
        Some(deployed + Duration::seconds(10)),
        Some(deployed + Duration::minutes(5)),
    )];
    let availability = Availability {
        requested: Window {
            from: deployed,
            to: deployed + Duration::minutes(1),
        },
        available_since: None,
        persisted_through: None,
        history_available: true,
        complete: false,
        gaps: vec![],
    };
    let result = evaluate(
        &q,
        Evidence {
            checks: &[],
            required_checks: &[],
            releases: &[],
            release_assessment: None,
            findings: &rows,
            baseline: &[],
            error_count: 1,
            failed_checks: 0,
            availability,
            next_cursor: None,
        },
        Utc::now(),
    )?;
    assert!(result.recovered_findings.is_empty());
    assert_eq!(result.new_findings.len(), 1);
    Ok(())
}
#[test]
fn reappeared_episode_is_new_and_unknown_start_stays_unclassified() -> Result<(), crate::Error> {
    let deployed = Utc::now() - Duration::minutes(10);
    let q = Deployment {
        deployed_at: deployed,
        window_seconds: Some(60),
        expected_revision: None,
        expected_digest: None,
        filter: Filter {
            target: Some("api".into()),
            ..Default::default()
        },
    };
    let baseline = [finding(
        deployed - Duration::seconds(50),
        Some(deployed - Duration::hours(1)),
        Some(deployed - Duration::seconds(5)),
    )];
    let mut unknown = finding(deployed + Duration::seconds(20), None, None);
    unknown.identity = "unknown".into();
    let rows = [
        finding(
            deployed + Duration::seconds(10),
            Some(deployed + Duration::seconds(10)),
            None,
        ),
        unknown,
    ];
    let availability = Availability {
        requested: Window {
            from: deployed,
            to: deployed + Duration::minutes(1),
        },
        available_since: None,
        persisted_through: None,
        history_available: true,
        complete: false,
        gaps: vec![],
    };
    let result = evaluate(
        &q,
        Evidence {
            checks: &[],
            required_checks: &[],
            releases: &[],
            release_assessment: None,
            findings: &rows,
            baseline: &baseline,
            error_count: 2,
            failed_checks: 0,
            availability,
            next_cursor: None,
        },
        Utc::now(),
    )?;
    assert_eq!(result.new_findings.len(), 1);
    assert_eq!(result.unclassified_findings.len(), 1);
    assert!(result.pre_existing_findings.is_empty());
    Ok(())
}

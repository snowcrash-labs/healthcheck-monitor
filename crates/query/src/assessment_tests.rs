//! Short deployment windows cannot pass using cached or unevaluated evidence.
use crate::{
    assessment::{Evidence, evaluate},
    enums::*,
    filter::{Deployment, Filter, Window},
    record::*,
    response::Availability,
};
use chrono::{Duration, Utc};
fn fixture() -> (Deployment, Record, Availability) {
    let at = Utc::now() - Duration::minutes(2);
    let query = Deployment {
        deployed_at: at,
        window_seconds: Some(60),
        expected_revision: None,
        expected_digest: None,
        filter: Filter {
            target: Some("api".into()),
            ..Default::default()
        },
    };
    let record = Record {
        id: "fixture".into(),
        identity: "api/Kubernetes".into(),
        scope: Scope {
            target: "api".into(),
            provider: Provider::Gcp,
            scope: "project".into(),
        },
        location: Location::default(),
        check: Some(Check::Kubernetes),
        resource: None,
        observed_at: at + Duration::seconds(45),
        last_observed_at: at + Duration::seconds(45),
        valid_until: Some(at + Duration::minutes(5)),
        closed_at: None,
        stale: false,
        details: Details::Check {
            required: true,
            started_at: at + Duration::seconds(30),
            finished_at: at + Duration::seconds(45),
            oldest_observation_at: Some(at + Duration::seconds(35)),
            complete: true,
            observations: 4,
            interval_seconds: 30,
            required_failures: 0,
            pending_observations: 0,
            operations: vec![],
            operations_truncated: false,
        },
    };
    let availability = Availability {
        requested: Window {
            from: at - Duration::minutes(1),
            to: at + Duration::minutes(1),
        },
        available_since: Some(at - Duration::days(1)),
        persisted_through: Some(Utc::now()),
        history_available: true,
        complete: true,
        gaps: vec![],
    };
    (query, record, availability)
}
fn verdict(
    query: &Deployment,
    checks: &[Record],
    releases: &[Record],
    availability: Availability,
    errors: u64,
    now: chrono::DateTime<Utc>,
) -> Result<Outcome, crate::Error> {
    Ok(evaluate(
        query,
        Evidence {
            checks,
            required_checks: &["api/Kubernetes".into()],
            releases,
            release_assessment: None,
            findings: &[],
            baseline: &[],
            error_count: errors,
            failed_checks: 0,
            availability,
            next_cursor: None,
        },
        now,
    )?
    .outcome)
}
#[test]
fn completed_short_window_with_fresh_evidence_passes() -> Result<(), crate::Error> {
    let (q, r, a) = fixture();
    assert_eq!(verdict(&q, &[r], &[], a, 0, Utc::now())?, Outcome::Passing);
    Ok(())
}
#[test]
fn cached_predeployment_observations_never_pass() -> Result<(), crate::Error> {
    let (q, mut r, a) = fixture();
    if let Details::Check {
        oldest_observation_at,
        ..
    } = &mut r.details
    {
        *oldest_observation_at = Some(q.deployed_at - Duration::seconds(1));
    }
    assert_eq!(
        verdict(&q, &[r], &[], a, 0, Utc::now())?,
        Outcome::Incomplete
    );
    Ok(())
}
#[test]
fn unfinished_window_and_missing_scans_are_pending() -> Result<(), crate::Error> {
    let (q, _, a) = fixture();
    assert_eq!(
        verdict(&q, &[], &[], a, 0, q.deployed_at + Duration::seconds(10))?,
        Outcome::Pending
    );
    Ok(())
}
#[test]
fn rollout_grace_and_unknown_capacity_do_not_establish_success() -> Result<(), crate::Error> {
    let (q, mut r, a) = fixture();
    if let Details::Check {
        pending_observations,
        ..
    } = &mut r.details
    {
        *pending_observations = 1;
    }
    assert_eq!(
        verdict(&q, &[r], &[], a, 0, Utc::now())?,
        Outcome::Incomplete
    );
    Ok(())
}
#[test]
fn persistence_gap_prevents_a_pass() -> Result<(), crate::Error> {
    let (q, r, mut a) = fixture();
    a.complete = false;
    assert_eq!(
        verdict(&q, &[r], &[], a, 0, Utc::now())?,
        Outcome::Incomplete
    );
    Ok(())
}
#[test]
fn errors_fail_even_before_window_finishes() -> Result<(), crate::Error> {
    let (q, r, a) = fixture();
    assert_eq!(
        verdict(&q, &[r], &[], a, 1, q.deployed_at + Duration::seconds(20))?,
        Outcome::Failing
    );
    Ok(())
}
#[test]
fn revision_requires_verified_observed_provenance() -> Result<(), crate::Error> {
    let (mut q, r, a) = fixture();
    q.expected_revision = Some("expected".into());
    let mut release = r.clone();
    release.identity = "api/image".into();
    release.details = Details::Release {
        revision: Some("expected".into()),
        observed_digests: vec!["sha256:aaa".into()],
        desired_digest: None,
        pending: false,
        verified: false,
        facts: vec![],
        links: vec![],
    };
    assert_eq!(
        verdict(
            &q,
            std::slice::from_ref(&r),
            &[release.clone()],
            a.clone(),
            0,
            Utc::now()
        )?,
        Outcome::Incomplete
    );
    if let Details::Release { verified, .. } = &mut release.details {
        *verified = true;
    }
    assert_eq!(
        verdict(&q, &[r], &[release], a, 0, Utc::now())?,
        Outcome::Passing
    );
    Ok(())
}
#[test]
fn wrong_running_digest_fails() -> Result<(), crate::Error> {
    let (mut q, r, a) = fixture();
    q.expected_digest = Some("sha256:expected".into());
    let mut release = r.clone();
    release.details = Details::Release {
        revision: None,
        observed_digests: vec!["sha256:wrong".into()],
        desired_digest: None,
        pending: false,
        verified: false,
        facts: vec![],
        links: vec![],
    };
    assert_eq!(
        verdict(&q, &[r], &[release], a, 0, Utc::now())?,
        Outcome::Failing
    );
    Ok(())
}

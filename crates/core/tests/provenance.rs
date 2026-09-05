//! Build recovery requires a later success for the same source and deployment target.
use chrono::{Duration, Utc};
use monitor_core::{model::*, provenance::mark_retries};
fn build(id: &str, revision: &str, state: ServiceState, offset: i64) -> Observation {
    Observation {
        resource: format!("dev/builds/{id}"),
        operation: "builds".into(),
        observed_at: Utc::now(),
        expected: Expected::Active,
        data: Data::Build {
            superseded: false,
            pipeline: "frontend".into(),
            revision: revision.into(),
            target: "web".into(),
            state,
            created_at: Some(Utc::now() + Duration::seconds(offset)),
        },
    }
}
#[test]
fn later_success_supersedes_matching_failure() {
    let mut observations = vec![
        build("failed", "abc1234", ServiceState::Failed, 0),
        build("retry", "abc1234", ServiceState::Ready, 60),
    ];
    mark_retries(&mut observations);
    assert!(matches!(
        observations[0].data,
        Data::Build {
            superseded: true,
            ..
        }
    ));
}
#[test]
fn different_revision_cannot_hide_failure() {
    let mut observations = vec![
        build("failed", "abc1234", ServiceState::Failed, 0),
        build("unrelated", "def5678", ServiceState::Ready, 60),
    ];
    mark_retries(&mut observations);
    assert!(matches!(
        observations[0].data,
        Data::Build {
            superseded: false,
            ..
        }
    ));
}
#[test]
fn earlier_success_cannot_hide_later_failure() {
    let mut observations = vec![
        build("failed", "abc1234", ServiceState::Failed, 60),
        build("old", "abc1234", ServiceState::Ready, 0),
    ];
    mark_retries(&mut observations);
    assert!(matches!(
        observations[0].data,
        Data::Build {
            superseded: false,
            ..
        }
    ));
}
#[test]
fn another_region_or_repository_cannot_hide_a_failed_build() {
    for (failed, success) in [
        (
            "build-details/us-east-1/failed",
            "build-details/us-west-2/success",
        ),
        ("workflows/org/backend", "workflows/org/frontend"),
    ] {
        let mut observations = vec![
            build("failed", "abc1234", ServiceState::Failed, 0),
            build("success", "abc1234", ServiceState::Ready, 60),
        ];
        observations[0].operation = failed.into();
        observations[1].operation = success.into();
        mark_retries(&mut observations);
        assert!(matches!(
            observations[0].data,
            Data::Build {
                superseded: false,
                ..
            }
        ));
    }
}

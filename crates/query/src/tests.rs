//! Boundary tests exercise time windows and aliases without cloud credentials.
use crate::{enums::*, filter::*};
use chrono::{Duration, Utc};

#[test]
fn aliases_cannot_cross_provider_scope() -> Result<(), Box<dyn std::error::Error>> {
    let mut filter = Filter {
        project: Some("project-a".into()),
        ..Default::default()
    };
    filter.normalize()?;
    assert_eq!(filter.provider, Some(Provider::Gcp));
    assert_eq!(filter.scope.as_deref(), Some("project-a"));
    filter.account = Some("account-b".into());
    assert!(filter.normalize().is_err());
    Ok(())
}
#[test]
fn explicit_window_preserves_offset_and_subminute_precision()
-> Result<(), Box<dyn std::error::Error>> {
    let mut filter: Filter = serde_json::from_str(
        r#"{"from":"2026-09-06T11:00:00.001-06:00","to":"2026-09-06T11:00:30.001-06:00"}"#,
    )?;
    filter.normalize()?;
    let window = filter.window(Utc::now())?;
    assert_eq!((window.to - window.from).num_milliseconds(), 30000);
    assert_eq!(window.from.to_rfc3339(), "2026-09-06T17:00:00.001+00:00");
    Ok(())
}
#[test]
fn relative_window_is_one_hour_by_default() -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now();
    let window = Filter::default().window(now)?;
    assert_eq!(window.to, now);
    assert_eq!(window.from, now - Duration::hours(1));
    Ok(())
}
#[test]
fn invalid_windows_are_rejected() {
    let now = Utc::now();
    for filter in [
        Filter {
            from: Some(now),
            to: Some(now),
            ..Default::default()
        },
        Filter {
            from: Some(now),
            lookback_seconds: Some(60),
            ..Default::default()
        },
        Filter {
            lookback_seconds: Some(0),
            ..Default::default()
        },
        Filter {
            lookback_seconds: Some(32 * 86400),
            ..Default::default()
        },
    ] {
        assert!(filter.window(now).is_err());
    }
}
#[test]
fn hostname_and_category_are_distinct() -> Result<(), Box<dyn std::error::Error>> {
    let mut filter = Filter {
        hostname: Some("API.Soundpatrol.com.".into()),
        check: Some(Check::Edge),
        ..Default::default()
    };
    filter.normalize()?;
    assert_eq!(filter.hostname.as_deref(), Some("api.soundpatrol.com"));
    assert_eq!(filter.check, Some(Check::Edge));
    Ok(())
}
#[test]
fn deployment_requires_scope_and_bounded_window() -> Result<(), Box<dyn std::error::Error>> {
    let mut query = Deployment {
        deployed_at: Utc::now(),
        window_seconds: Some(60),
        expected_revision: None,
        expected_digest: None,
        filter: Default::default(),
    };
    assert!(query.window().is_err());
    query.filter.target = Some("api".into());
    assert_eq!((query.window()?.to - query.deployed_at).num_seconds(), 60);
    query.window_seconds = Some(59);
    assert!(query.window().is_err());
    Ok(())
}
#[test]
fn unknown_fields_and_enum_values_fail_closed() {
    for source in [
        r#"{"command":"run"}"#,
        r#"{"check":"shell"}"#,
        r#"{"provider":"anything"}"#,
    ] {
        assert!(serde_json::from_str::<Filter>(source).is_err());
    }
}
#[test]
fn deployment_flattened_filter_round_trips() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"{"deployed_at":"2026-09-06T17:00:00Z","target":"api","window_seconds":60}"#;
    let value: Deployment = serde_json::from_str(source)?;
    assert_eq!(value.filter.target.as_deref(), Some("api"));
    Ok(())
}

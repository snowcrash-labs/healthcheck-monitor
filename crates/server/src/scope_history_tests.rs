//! Historical failures preserve current metadata, known watermarks and one shared deadline.
use std::time::Duration;

#[test]
fn failed_scope_lookup_invalidates_successful_watermark_availability() {
    let now = chrono::Utc::now();
    let availability = monitor_query::response::Availability {
        requested: monitor_query::filter::Window {
            from: now - chrono::Duration::minutes(1),
            to: now,
        },
        available_since: Some(now - chrono::Duration::days(1)),
        persisted_through: Some(now),
        history_available: true,
        complete: true,
        gaps: vec![],
    };
    let degraded = crate::scope_api::history_failed(availability, "scope query failed");
    assert!(!degraded.history_available);
    assert!(!degraded.complete);
    assert_eq!(degraded.persisted_through, Some(now));
    assert_eq!(degraded.gaps, vec!["scope query failed"]);
}

#[tokio::test]
async fn scope_lookup_failure_after_successful_watermark_keeps_current_scope()
-> Result<(), Box<dyn std::error::Error>> {
    let (app, _) = crate::test_support::app(&crate::config::Config::default()).await?;
    let now = chrono::Utc::now();
    let window = monitor_query::filter::Window {
        from: now - chrono::Duration::minutes(1),
        to: now,
    };
    let availability = monitor_query::response::Availability {
        requested: window,
        available_since: Some(window.from),
        persisted_through: Some(now),
        history_available: true,
        complete: true,
        gaps: vec![],
    };
    let result = crate::scope_api::assemble(
        &app,
        &Default::default(),
        window,
        None,
        async { availability },
        async { Err(monitor_history::error::Error::Pool) },
    )
    .await
    .map_err(|_| "scope fallback failed")?;
    assert_eq!(result.items.len(), 1);
    assert!(result.items[0].current);
    assert!(!result.availability.history_available);
    assert!(!result.availability.complete);
    assert!(
        result
            .availability
            .gaps
            .iter()
            .any(|gap| gap.contains("retained scopes may be missing"))
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn stalled_history_has_a_bounded_current_scope_fallback()
-> Result<(), Box<dyn std::error::Error>> {
    let (app, _) = crate::test_support::app(&crate::config::Config::default()).await?;
    let now = chrono::Utc::now();
    let window = monitor_query::filter::Window {
        from: now - chrono::Duration::minutes(1),
        to: now,
    };
    let start = tokio::time::Instant::now();
    let result = crate::scope_api::assemble(
        &app,
        &Default::default(),
        window,
        None,
        std::future::pending(),
        std::future::pending(),
    )
    .await
    .map_err(|_| "scope fallback failed")?;
    assert_eq!(start.elapsed(), Duration::from_secs(2));
    assert_eq!(result.items.len(), 1);
    assert!(!result.availability.complete);
    assert!(
        result
            .availability
            .gaps
            .iter()
            .any(|gap| gap.contains("timed out"))
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn scope_timeout_preserves_completed_watermark_within_the_shared_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let (app, _) = crate::test_support::app(&crate::config::Config::default()).await?;
    let now = chrono::Utc::now();
    let window = monitor_query::filter::Window {
        from: now - chrono::Duration::minutes(1),
        to: now,
    };
    let availability = monitor_query::response::Availability {
        requested: window,
        available_since: Some(window.from),
        persisted_through: Some(now),
        history_available: true,
        complete: true,
        gaps: vec![],
    };
    let start = tokio::time::Instant::now();
    let result = crate::scope_api::assemble(
        &app,
        &Default::default(),
        window,
        None,
        async {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            availability
        },
        async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok(vec![])
        },
    )
    .await
    .map_err(|_| "scope fallback failed")?;
    assert_eq!(start.elapsed(), Duration::from_secs(2));
    assert_eq!(result.availability.available_since, Some(window.from));
    assert_eq!(result.availability.persisted_through, Some(now));
    assert!(!result.availability.complete);
    Ok(())
}

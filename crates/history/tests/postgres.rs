//! Live PostgreSQL contract tests run only against an explicitly selected isolated test database.
use monitor_core::model::*;
use monitor_history::{
    History,
    config::Config,
    enums,
    query::Filter,
    records::{Event, Run},
    types::Digest,
};
use std::sync::Arc;

#[tokio::test]
#[ignore = "requires HEALTHCHECK_TEST_DATABASE_URL for an isolated PostgreSQL 18.6 database"]
async fn native_uuid_keys_idempotency_typed_history_and_retention()
-> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("HEALTHCHECK_TEST_DATABASE_URL")?;
    let parsed: tokio_postgres::Config = url.parse()?;
    if !parsed
        .get_dbname()
        .is_some_and(|name| name == "healthcheck_monitor_dashboard_test")
    {
        return Err("refusing non-test database".into());
    }
    let config = Config {
        event_rows: 3,
        run_rows: 3,
        configuration_rows: 32,
        ..Default::default()
    };
    let history: Arc<History> = History::new(url, config)?;
    history.migrate().await?;
    let revision = Digest::try_new("a".repeat(64))?;
    let now = chrono::Utc::now();
    let mut events = Vec::new();
    for index in 0..5 {
        let at = now + chrono::Duration::milliseconds(index);
        let finding = Finding {
            diagnostic: None,
            check: Some(Check::Kubernetes),
            id: format!("fixture-{index}"),
            resource: format!("fixture/pod-{index}"),
            rule: "pod-not-ready".into(),
            severity: Severity::Error,
            evidence: vec!["pods".into()],
            observed_at: at,
            expected: Expected::Active,
            confidence: Confidence::Direct,
            stale: false,
            clear_count: 0,
            valid_until: None,
        };
        events.push(Event::new(
            "fixture",
            &Transition {
                at,
                finding: finding.id.clone(),
                kind: TransitionKind::New,
            },
            &finding,
        )?);
    }
    let result = CheckResult::failure(
        "fixture".into(),
        Check::Kubernetes,
        "a".repeat(64),
        Coverage::Denied,
    );
    let runs = vec![Run::new(&result)?];
    history.write(&revision, &runs, &events, &[]).await?;
    let filter = Filter {
        target: Some(monitor_history::types::Name::try_new(
            "fixture".to_string(),
        )?),
        limit: 100,
        ..Default::default()
    };
    let first = history.events(&filter).await?;
    assert_eq!(first.len(), 3);
    assert!(
        first
            .iter()
            .all(|row| row.finding_event_id.as_ref().get_version_num() == 7)
    );
    assert_eq!(first[0].finding_event_kind, enums::Kind::New);
    let ids: Vec<_> = first.iter().map(|row| row.finding_event_id).collect();
    history.write(&revision, &runs, &events, &[]).await?;
    let second = history.events(&filter).await?;
    assert_eq!(
        second
            .iter()
            .map(|row| row.finding_event_id)
            .collect::<Vec<_>>(),
        ids
    );
    let run_rows = history.runs(None, None, None, 100).await?;
    assert!(
        run_rows
            .iter()
            .all(|row| row.check_run_id.as_ref().get_version_num() == 7)
    );
    let next = Filter {
        before: Some((second[0].finding_event_at, second[0].finding_event_id)),
        ..filter
    };
    assert_eq!(history.events(&next).await?.len(), 2);
    let stop = tokio_util::sync::CancellationToken::new();
    let (journal, writer) = monitor_history::journal::Journal::start(history.clone(), stop.clone());
    journal.submit(
        revision,
        [Run::new(&result)],
        [Err(monitor_history::error::Error::Record)],
    );
    assert!(
        journal
            .flush(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await
    );
    assert!(journal.health().available);
    assert_eq!(journal.health().dropped_events, 1);
    assert!(history.gaps().await? > 0);
    stop.cancel();
    writer.await?;
    Ok(())
}

#[test]
fn application_values_reject_invalid_uuid_versions_and_match_text_bounds() {
    assert!(monitor_history::types::Id::try_new(uuid::Uuid::nil()).is_err());
    assert!(monitor_history::types::Name::try_new("가".repeat(128)).is_ok());
    assert!(monitor_history::types::Name::try_new("가".repeat(129)).is_err());
    assert!(monitor_history::types::Resource::try_new("x\0y".to_string()).is_err());
    assert!(Digest::try_new("Z".repeat(64)).is_err());
}

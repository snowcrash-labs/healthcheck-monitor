//! PostgreSQL query contracts verify intervals, exact filters, idempotent updates, and retention.
use monitor_history::{History, config::Config, query_records::QueryRecord, records::digest};
use monitor_query::{
    enums::*,
    filter::{Filter, Window},
    record::*,
};

fn finding(now: chrono::DateTime<chrono::Utc>) -> Record {
    Record {
        id: String::new(),
        identity: "query-contract/pod-not-ready".into(),
        scope: Scope {
            target: "query-contract".into(),
            provider: Provider::Gcp,
            scope: "project-query-contract".into(),
        },
        location: Location {
            namespace: Some("app".into()),
            cluster: Some("cluster-a".into()),
            ..Default::default()
        },
        check: Some(Check::Kubernetes),
        resource: Some("query-contract/pod".into()),
        observed_at: now - chrono::Duration::hours(6),
        last_observed_at: now,
        valid_until: Some(now + chrono::Duration::minutes(2)),
        closed_at: None,
        stale: false,
        details: Details::Finding {
            rule: "pod-not-ready".into(),
            severity: Severity::Error,
            state: FindingState::Active,
            first_detected_at: Some(now - chrono::Duration::hours(6)),
            expected: Expected::Active,
            confidence: Confidence::Direct,
            facts: vec![Fact {
                label: "Exit code".into(),
                value: "137".into(),
            }],
            links: vec![],
            legacy: false,
        },
    }
}
#[tokio::test]
#[ignore = "requires isolated HEALTHCHECK_TEST_DATABASE_URL"]
async fn history_queries_keep_recovered_evidence_and_scope()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = DATABASE_LOCK.lock().await;
    let url = std::env::var("HEALTHCHECK_TEST_DATABASE_URL")?;
    let parsed: tokio_postgres::Config = url.parse()?;
    if parsed.get_dbname() != Some("healthcheck_monitor_dashboard_test") {
        return Err("refusing non-test database".into());
    }
    let history = History::new(url, Config::default())?;
    history.migrate().await?;
    let now = chrono::Utc::now();
    let mut record = finding(now);
    let target = format!("query-contract-{}", now.timestamp_micros());
    record.scope.target = target.clone();
    record.identity = format!("{target}/pod-not-ready");
    record.resource = Some(format!("{target}/pod"));
    let key = digest(&record.identity)?;
    let revision = digest(&"query-contract-revision")?;
    let row = QueryRecord::new(key.clone(), record.clone())?;
    history
        .write_queries(&revision, &[], &[], &[], &[row.clone(), row])
        .await?;
    let window = Window {
        from: now - chrono::Duration::minutes(1),
        to: now + chrono::Duration::seconds(1),
    };
    let filter = Filter {
        target: Some(target),
        scope: Some("project-query-contract".into()),
        namespace: Some("app".into()),
        ..Default::default()
    };
    let rows = history
        .query_page(&filter, window, Some(Category::Finding), None)
        .await?;
    assert_eq!(rows.len(), 1);
    let id = rows[0].id.clone();
    assert_eq!(id.parse::<uuid::Uuid>()?.get_version_num(), 7);
    assert_eq!(
        rows[0].observed_at.timestamp_micros(),
        record.observed_at.timestamp_micros()
    );
    let wrong = Filter {
        namespace: Some("missing".into()),
        ..filter.clone()
    };
    assert!(
        history
            .query_page(&wrong, window, None, None)
            .await?
            .is_empty()
    );
    assert_eq!(
        history
            .query_count(&filter, window, Category::Finding, Some(Severity::Error))
            .await?,
        1
    );
    let mut closed = record;
    closed.closed_at = Some(now);
    if let Details::Finding { state, .. } = &mut closed.details {
        *state = FindingState::Recovered;
    }
    history
        .write_queries(&revision, &[], &[], &[], &[QueryRecord::new(key, closed)?])
        .await?;
    let recovered = history.query_page(&filter, window, None, None).await?;
    assert_eq!(recovered[0].id, id);
    assert_eq!(recovered[0].state(), Some(FindingState::Recovered));
    assert!(
        matches!(&recovered[0].details,Details::Finding { facts,.. } if facts.iter().any(|f|f.value=="137"))
    );
    let active = Filter {
        state: Some(FindingState::Active),
        ..filter
    };
    assert!(
        history
            .query_page(&active, window, None, None)
            .await?
            .is_empty()
    );
    Ok(())
}
#[test]
fn forbidden_fields_cannot_decode_into_query_evidence() -> Result<(), Box<dyn std::error::Error>> {
    let mut value = serde_json::to_value(finding(chrono::Utc::now()))?;
    value["customer_payload"] = serde_json::json!({"token":"secret"});
    assert!(serde_json::from_value::<Record>(value).is_err());
    let mut record = finding(chrono::Utc::now());
    record.location.native_id = Some("a".repeat(5000));
    assert!(QueryRecord::new(digest(&"oversized")?, record).is_err());
    Ok(())
}

static DATABASE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
async fn connection(config: Config) -> Result<std::sync::Arc<History>, Box<dyn std::error::Error>> {
    let url = std::env::var("HEALTHCHECK_TEST_DATABASE_URL")?;
    let parsed: tokio_postgres::Config = url.parse()?;
    if parsed.get_dbname() != Some("healthcheck_monitor_dashboard_test") {
        return Err("refusing non-test database".into());
    }
    let history = History::new(url, config)?;
    history.migrate().await?;
    Ok(history)
}
#[tokio::test]
#[ignore = "requires isolated HEALTHCHECK_TEST_DATABASE_URL"]
async fn missing_observation_windows_survive_collection_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = DATABASE_LOCK.lock().await;
    let history = connection(Config::default()).await?;
    let now = chrono::Utc::now();
    let mut rows = vec![];
    for seconds in [0, 600, 630] {
        let at = now - chrono::Duration::hours(1) + chrono::Duration::seconds(seconds);
        let mut record = finding(at);
        record.identity = "gap-contract/Kubernetes".into();
        record.scope.target = "gap-contract".into();
        record.observed_at = at;
        record.last_observed_at = at;
        record.valid_until = Some(at + chrono::Duration::seconds(90));
        record.resource = None;
        record.details = Details::Check {
            required: true,
            started_at: at,
            finished_at: at,
            oldest_observation_at: Some(at),
            complete: true,
            observations: 1,
            interval_seconds: 30,
            required_failures: 0,
            pending_observations: 0,
            operations: vec![],
            operations_truncated: false,
        };
        rows.push(QueryRecord::new(digest(&("gap-contract", at))?, record)?);
    }
    history
        .write_queries(&digest(&"gap-revision")?, &[], &[], &[], &rows)
        .await?;
    let filter = Filter {
        target: Some("gap-contract".into()),
        ..Default::default()
    };
    let window = Window {
        from: now - chrono::Duration::minutes(58),
        to: now - chrono::Duration::minutes(55),
    };
    assert!(history.query_gap_count(&filter, window).await? > 0);
    let latest = history
        .query_latest(
            &filter,
            Window {
                from: now - chrono::Duration::hours(2),
                to: now,
            },
            Category::Check,
            None,
            None,
        )
        .await?;
    assert_eq!(latest.len(), 1);
    let other = Filter {
        target: Some("independent-contract".into()),
        ..Default::default()
    };
    assert_eq!(history.query_gap_count(&other, window).await?, 0);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated HEALTHCHECK_TEST_DATABASE_URL"]
async fn synthetic_week_of_churn_respects_retention_and_reports_eviction()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = DATABASE_LOCK.lock().await;
    let history = connection(Config {
        diagnostic_rows: 128,
        ..Default::default()
    })
    .await?;
    let now = chrono::Utc::now();
    let revision = digest(&"retention-contract")?;
    for day in (0..8).rev() {
        let mut rows = vec![];
        for index in 0..256 {
            let at = now - chrono::Duration::days(day) + chrono::Duration::milliseconds(index);
            let mut record = finding(at);
            record.scope.target = "retention-contract".into();
            record.identity = format!("retention-contract/{day}/{index}");
            record.observed_at = at;
            record.last_observed_at = at;
            record.closed_at = Some(at);
            record.valid_until = Some(at);
            rows.push(QueryRecord::new(digest(&record.identity)?, record)?);
        }
        history
            .write_queries(&revision, &[], &[], &[], &rows)
            .await?;
    }
    let filter = Filter {
        target: Some("retention-contract".into()),
        ..Default::default()
    };
    let window = Window {
        from: now - chrono::Duration::days(7),
        to: now + chrono::Duration::seconds(1),
    };
    assert!(
        history
            .query_count(&filter, window, Category::Finding, None)
            .await?
            <= 128
    );
    let availability = history.query_availability(window).await?;
    assert!(!availability.complete);
    assert!(
        availability
            .available_since
            .is_some_and(|at| at > window.from)
    );
    Ok(())
}

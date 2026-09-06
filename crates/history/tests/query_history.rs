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
            expected: "Active".into(),
            confidence: "Direct".into(),
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
    let url = std::env::var("HEALTHCHECK_TEST_DATABASE_URL")?;
    let parsed: tokio_postgres::Config = url.parse()?;
    if parsed.get_dbname() != Some("healthcheck_monitor_dashboard_test") {
        return Err("refusing non-test database".into());
    }
    let history = History::new(url, Config::default())?;
    history.migrate().await?;
    let now = chrono::Utc::now();
    let record = finding(now);
    let key = digest(&"query-contract")?;
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
        target: Some("query-contract".into()),
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
    assert_eq!(rows[0].observed_at, record.observed_at);
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

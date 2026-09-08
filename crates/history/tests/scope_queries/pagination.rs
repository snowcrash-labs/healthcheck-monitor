//! Historical scope pages retain lexicographic boundaries and exclude out-of-window records.
use crate::{DATABASE_LOCK, connection, finding};
use monitor_history::{config::Config, query_records::QueryRecord, records::digest};
use monitor_query::{
    enums::FindingState,
    filter::{Filter, Window},
    record::Details,
};

#[tokio::test]
#[ignore = "requires isolated HEALTHCHECK_TEST_DATABASE_URL"]
async fn correlated_scope_pages_keep_every_matching_scope_once()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = DATABASE_LOCK.lock().await;
    let history = connection(Config::default()).await?;
    let now = chrono::Utc::now();
    let target = format!("scope-contract-{}", now.timestamp_micros());
    let mut records = vec![];
    for index in 0..8 {
        for observation in 0..4 {
            let mut row = finding(now);
            row.scope.target = target.clone();
            row.scope.scope = format!("scope-{index}");
            row.identity = format!("{target}/{index}/{observation}");
            records.push(QueryRecord::new(digest(&row.identity)?, row)?);
        }
    }
    let mut old = finding(now - chrono::Duration::hours(6));
    old.scope.target = target.clone();
    old.scope.scope = "scope-expired".into();
    old.identity = format!("{target}/expired");
    old.closed_at = Some(now - chrono::Duration::hours(5));
    if let Details::Finding { state, .. } = &mut old.details {
        *state = FindingState::Recovered;
    }
    records.push(QueryRecord::new(digest(&old.identity)?, old)?);
    history
        .write_queries(&digest(&target)?, &[], &[], &[], &records)
        .await?;
    let filter = Filter {
        target: Some(target),
        limit: Some(3),
        ..Default::default()
    };
    let window = Window {
        from: now - chrono::Duration::minutes(1),
        to: now + chrono::Duration::seconds(1),
    };
    let mut after = None;
    let mut actual = vec![];
    for _ in 0..4 {
        let mut page = history
            .query_scopes(&filter, window, after.as_ref())
            .await?;
        assert!(page.len() <= 4);
        let more = page.len() > 3;
        page.truncate(3);
        after = page.last().cloned();
        actual.extend(page.into_iter().map(|scope| scope.scope));
        if !more {
            break;
        }
    }
    assert_eq!(
        actual,
        (0..8)
            .map(|index| format!("scope-{index}"))
            .collect::<Vec<_>>()
    );
    Ok(())
}

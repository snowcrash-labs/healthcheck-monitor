//! Credential cache retention is independent of the evidence schema.
use monitor_core::budget::Budget as Semaphore;
use std::sync::Arc;
#[tokio::test(start_paused = true)]
async fn token_expiry_and_failed_cache_admission_do_not_retain_unbounded_credentials() {
    let tokens = super::registry_tokens::Tokens::new(Arc::new(Semaphore::new(65536)));
    tokens
        .put("registry".into(), "private-token".into(), 1)
        .await;
    assert!(tokens.get("registry").await.is_some());
    tokio::time::advance(std::time::Duration::from_secs(2)).await;
    assert!(tokens.get("registry").await.is_none());
    tokens.clear().await;
    let no_budget = super::registry_tokens::Tokens::new(Arc::new(Semaphore::new(0)));
    no_budget
        .put("registry".into(), "private-token".into(), 300)
        .await;
    assert!(no_budget.get("registry").await.is_none());
}

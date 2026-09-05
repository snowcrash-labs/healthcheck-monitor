//! Registry credentials use a bounded cache and are never serialized or logged.
use monitor_core::budget::{Budget, Permit};
use std::{sync::Arc, time::Duration};
struct Token {
    value: String,
    until: tokio::time::Instant,
    _bytes: Permit,
}
pub struct Tokens {
    entries: scc::HashCache<String, Token>,
    bytes: Arc<Budget>,
}
impl Tokens {
    pub async fn clear(&self) {
        self.entries.clear_async().await;
    }
    pub fn new(bytes: Arc<Budget>) -> Self {
        Self {
            entries: scc::HashCache::with_capacity(0, 128),
            bytes,
        }
    }
    pub async fn get(&self, key: &str) -> Option<String> {
        self.entries
            .read_async(key, |_, token| {
                (token.until > tokio::time::Instant::now()).then(|| token.value.clone())
            })
            .await
            .flatten()
    }
    pub async fn put(&self, key: String, value: String, seconds: u64) {
        let _ = self.entries.remove_async(&key).await;
        let Ok(bytes) = u32::try_from(value.len().saturating_mul(4).saturating_add(512)) else {
            return;
        };
        let Ok(permit) = self.bytes.clone().try_acquire_many_owned(bytes) else {
            return;
        };
        let token = Token {
            value,
            until: tokio::time::Instant::now() + Duration::from_secs(seconds.clamp(1, 300)),
            _bytes: permit,
        };
        let _ = self.entries.entry_async(key).await.or_put(token);
    }
}

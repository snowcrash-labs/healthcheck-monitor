//! Credential-free HTTP pools are shared by compatible transport settings.
use crate::transport::Error;
use monitor_core::config::settings::Settings;
use std::{sync::Arc, time::Duration};
#[cfg(test)]
#[path = "http_pool_tests.rs"]
mod tests;

pub struct Pools {
    clients: scc::HashCache<(u64, usize), reqwest::Client>,
    pub(crate) network: Arc<crate::dns::Network>,
}
impl Default for Pools {
    fn default() -> Self {
        Self {
            clients: scc::HashCache::with_capacity(0, 128),
            network: Arc::new(crate::dns::Network::default()),
        }
    }
}
impl Pools {
    /// No default authorization headers, cookies, or request deadlines enter a shared pool.
    pub fn client(&self, settings: &Settings) -> Result<reqwest::Client, Error> {
        let key = (settings.connect_timeout.0, settings.concurrency);
        if let Some(entry) = self.clients.get_sync(&key) {
            return Ok(entry.get().clone());
        }
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(settings.connect_timeout.duration())
            .pool_max_idle_per_host(settings.concurrency)
            .pool_idle_timeout(Duration::from_secs(60))
            .tcp_keepalive(Duration::from_secs(60))
            .tcp_nodelay(true)
            .http2_adaptive_window(true)
            .dns_resolver(self.network.clone())
            .tls_info(true)
            .user_agent("Soundpatrol-healthcheck-monitor/0.1")
            .build()
            .map_err(|_| Error::Unavailable)?;
        let (_, entry) = self.clients.entry_sync(key).or_put(client);
        Ok(entry.get().clone())
    }
}

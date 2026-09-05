//! Atomic credential generations and resizeable shared budgets.
use super::*;
impl Router {
    pub fn new(config: Config, effective: &Effective) -> Self {
        let settings = effective
            .jobs
            .first()
            .map(|job| job.settings.clone())
            .unwrap_or_default();
        Self {
            pools: Arc::new(monitor_integrations::http_pool::Pools::default()),
            remote: monitor_integrations::admission::Limits::new(effective),
            config: RwLock::new(config),
            revision: RwLock::new(effective.revision.clone()),
            scopes: scc::HashMap::new(),
            processes: Processes::new(settings.subprocesses),
            cache_bytes: Arc::new(Budget::new(settings.memory_bytes / 4)),
            log_cursors: scc::HashMap::new(),
            discovery_auth: scc::HashCache::with_capacity(0, 128),
        }
    }
    /// Holding the configuration write lock prevents obsolete work from repopulating caches.
    pub async fn reload(&self, config: Config, effective: &Effective) {
        let mut current = self.config.write().await;
        *current = config;
        *self.revision.write().await = effective.revision.clone();
        self.remote.reload(effective).await;
        if let Some(job) = effective.jobs.first() {
            self.cache_bytes.resize(job.settings.memory_bytes / 4);
            self.processes.resize(job.settings.subprocesses);
        }
        self.scopes.clear_async().await;
        self.log_cursors.clear_async().await;
        self.discovery_auth.clear_async().await;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn reload_rejects_obsolete_jobs_and_retains_charges_from_old_clients()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut config = Config::parse(
            "version=1\n[[targets]]\nname='dev'\nprovider='gcp'\nscope='project'\nregions=['us-central1']",
        )?;
        let selected = monitor_core::config::resolve::Selection::default();
        let effective = config.resolve(&selected)?;
        let router = Router::new(config.clone(), &effective);
        let reservation = router
            .cache_bytes
            .clone()
            .try_acquire_many_owned(48 * 1024 * 1024)?;
        config.settings.memory_bytes = Some(128 * 1024 * 1024);
        let replacement = config.resolve(&selected)?;
        router.reload(config, &replacement).await;
        assert_eq!(router.cache_bytes.available_permits(), 0);
        assert!(matches!(
            router
                .scope(effective.jobs.first().ok_or("missing job")?)
                .await,
            Err(Error::Cancelled)
        ));
        drop(reservation);
        assert_eq!(router.cache_bytes.available_permits(), 32 * 1024 * 1024);
        assert!(
            router
                .scope(replacement.jobs.first().ok_or("missing job")?)
                .await
                .is_ok()
        );
        Ok(())
    }
}

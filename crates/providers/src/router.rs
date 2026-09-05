//! Shared target clients and credential recovery without interactive collection.
use crate::auth::Auth;
use monitor_core::{
    config::{resolve::Job, types::Config},
    model::*,
    scheduler::Collector,
};
use monitor_integrations::{
    kubernetes::Kubernetes,
    nats::Nats,
    process::Processes,
    transport::{Error, Http},
};
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedSemaphorePermit, RwLock, Semaphore};
use tokio_util::sync::CancellationToken;

pub(crate) struct Scope {
    log_dedupe: Mutex<Option<Arc<monitor_integrations::log_dedup::Dedupe>>>,
    pub(crate) inventory: crate::inventory_cache::InventoryCache,
    pub(crate) http: Http,
    auth: Mutex<Option<Arc<Auth>>>,
    pub(crate) kube: Mutex<Option<Kubernetes>>,
    kube_cache: Mutex<Option<Cached>>,
    pub(crate) github_cache: Mutex<Option<Cached>>,
    pub(crate) nats: Mutex<Option<Nats>>,
}
pub struct Router {
    pub(crate) config: RwLock<Config>,
    scopes: scc::HashMap<String, Arc<Scope>>,
    pub(crate) processes: Processes,
    pub(crate) cache_bytes: Arc<Semaphore>,
    log_cursors: scc::HashMap<String, chrono::DateTime<chrono::Utc>>,
}
pub(crate) struct Cached {
    pub(crate) consumers: std::collections::BTreeSet<Check>,
    pub(crate) result: CheckResult,
    pub(crate) _bytes: OwnedSemaphorePermit,
}
impl Router {
    pub fn new(config: Config, subprocesses: usize) -> Self {
        let bytes = config.settings.memory_bytes.unwrap_or(256 * 1024 * 1024) / 4;
        Self {
            config: RwLock::new(config),
            scopes: scc::HashMap::new(),
            processes: Processes::new(subprocesses),
            cache_bytes: Arc::new(Semaphore::new(bytes)),
            log_cursors: scc::HashMap::new(),
        }
    }
    pub async fn reload(&self, config: Config) {
        *self.config.write().await = config;
        self.scopes.clear_async().await;
        self.log_cursors.clear_async().await;
    }
    /// Restore completed log windows without changing their evidence timestamps.
    pub async fn restore_logs(&self, snapshot: &Snapshot) {
        for result in snapshot
            .results
            .values()
            .filter(|result| result.check == Check::Logs && crate::log_cursor::complete(result))
            .take(128)
        {
            let end = result
                .observations
                .iter()
                .filter_map(|obs| match obs.data {
                    Data::LogWindow {
                        end,
                        complete: true,
                        ..
                    } => Some(end),
                    _ => None,
                })
                .min();
            if let Some(end) = end {
                let _ = self
                    .log_cursors
                    .insert_async(result.target.clone(), end)
                    .await;
            }
        }
    }
    pub(crate) async fn scope(&self, job: &Job) -> Result<Arc<Scope>, Error> {
        let config = self.config.read().await;
        if !config.targets.iter().any(|target| {
            target.name == job.target.name
                && target.scope == job.target.scope
                && target.provider == job.target.provider
        }) {
            return Err(Error::Cancelled);
        }
        if let Some(scope) = self
            .scopes
            .read_async(&job.target.name, |_, scope| scope.clone())
            .await
        {
            return Ok(scope);
        }
        if self.scopes.len() >= 128 {
            return Err(Error::Limit);
        }
        let scope = Arc::new(Scope {
            log_dedupe: Mutex::new(None),
            inventory: crate::inventory_cache::InventoryCache::new(
                job.settings.ready_queue,
                self.cache_bytes.clone(),
            ),
            http: Http::new(&job.settings)?,
            auth: Mutex::new(None),
            kube: Mutex::new(None),
            kube_cache: Mutex::new(None),
            github_cache: Mutex::new(None),
            nats: Mutex::new(None),
        });
        let entry = self
            .scopes
            .entry_async(job.target.name.clone())
            .await
            .or_insert(scope);
        Ok(entry.get().clone())
    }
    pub(crate) async fn auth(&self, job: &Job, scope: &Scope) -> Result<Arc<Auth>, Error> {
        let mut auth = scope.auth.lock().await;
        if let Some(auth) = auth.as_ref() {
            return Ok(auth.clone());
        }
        let profile = {
            let config = self.config.read().await;
            job.target
                .credential
                .as_ref()
                .and_then(|name| config.credentials.get(name))
                .cloned()
        };
        let loaded = tokio::time::timeout(
            job.settings.operation_timeout.duration(),
            Auth::new(
                job.target.provider,
                profile.as_ref(),
                job.target.regions.first().map(String::as_str),
                &job.target.scope,
                &scope.http,
                &job.settings,
            ),
        )
        .await
        .map_err(|_| Error::Timeout)??;
        let loaded = Arc::new(loaded);
        *auth = Some(loaded.clone());
        Ok(loaded)
    }
    pub(crate) async fn log_dedupe(
        &self,
        job: &Job,
        scope: &Scope,
    ) -> Option<Arc<monitor_integrations::log_dedup::Dedupe>> {
        if !job.continuous {
            return None;
        }
        let mut cache = scope.log_dedupe.lock().await;
        if cache.is_none() {
            *cache = monitor_integrations::log_dedup::Dedupe::new(
                job.settings.log_dedup_entries,
                self.cache_bytes.clone(),
            )
            .map(Arc::new);
        }
        cache.clone()
    }
    pub(crate) async fn kube(
        &self,
        job: &Job,
        scope: &Scope,
        cancel: &CancellationToken,
    ) -> Result<CheckResult, Error> {
        let mut cache = scope.kube_cache.lock().await;
        if let Some(cached) = cache.as_mut().filter(|cached| {
            cached.result.revision == job.revision
                && !cached.consumers.contains(&job.check)
                && (chrono::Utc::now() - cached.result.finished_at).num_seconds() < 30
        }) {
            cached.consumers.insert(job.check);
            let mut result = cached.result.clone();
            result.check = job.check;
            return Ok(result);
        }
        let mut kube = scope.kube.lock().await;
        if kube.is_none() {
            *kube = Some(
                tokio::time::timeout(
                    job.settings.operation_timeout.duration(),
                    Kubernetes::new(job.target.context.as_deref()),
                )
                .await
                .map_err(|_| Error::Timeout)??,
            );
        }
        let kube = kube.as_ref().ok_or(Error::Authentication)?.clone();
        let result = kube.collect(job, cancel).await;
        *cache = None;
        let bytes = monitor_core::bounds::result_bytes(&result);
        if let Ok(bytes) = u32::try_from(bytes)
            && let Ok(permit) = self.cache_bytes.clone().try_acquire_many_owned(bytes)
        {
            *cache = Some(Cached {
                consumers: std::collections::BTreeSet::from([job.check]),
                result: result.clone(),
                _bytes: permit,
            });
        }
        Ok(result)
    }
}

impl Collector for Router {
    async fn collect(&self, job: &Job, cancel: CancellationToken) -> CheckResult {
        let mut selected = job.clone();
        if job.check == Check::Logs {
            selected.log_end = Some(chrono::Utc::now());
            if job.continuous {
                selected.log_start = self
                    .log_cursors
                    .read_async(&job.target.name, |_, at| *at)
                    .await;
            }
        }
        let result = tokio::select! {
            _ = cancel.cancelled() => CheckResult::failure(job.target.name.clone(), job.check, job.revision.clone(), Coverage::Cancelled),
            result = self.execute(&selected, &cancel) => match result {
                Ok(result) => result,
                Err(error) => CheckResult::failure(job.target.name.clone(), job.check, job.revision.clone(), error.coverage()),
            }
        };
        if job.continuous
            && job.check == Check::Logs
            && crate::log_cursor::complete(&result)
            && let Some(end) = selected.log_end
        {
            self.log_cursors
                .entry_async(job.target.name.clone())
                .await
                .or_insert(end)
                .get_mut()
                .clone_from(&end);
        }
        result
    }
}
pub(crate) fn base(job: &Job) -> CheckResult {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    result.operations.clear();
    result
}

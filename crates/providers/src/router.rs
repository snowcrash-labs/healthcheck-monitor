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
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

pub(crate) struct Scope {
    pub(crate) http: Http,
    auth: Mutex<Option<Arc<Auth>>>,
    pub(crate) kube: Mutex<Option<Kubernetes>>,
    kube_cache: Mutex<Option<CheckResult>>,
    pub(crate) nats: Mutex<Option<Nats>>,
}
pub struct Router {
    pub(crate) config: RwLock<Config>,
    scopes: scc::HashMap<String, Arc<Scope>>,
    pub(crate) processes: Processes,
}
impl Router {
    pub fn new(config: Config, subprocesses: usize) -> Self {
        Self {
            config: RwLock::new(config),
            scopes: scc::HashMap::new(),
            processes: Processes::new(subprocesses),
        }
    }
    pub async fn reload(&self, config: Config) {
        *self.config.write().await = config;
        self.scopes.clear_async().await;
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
            http: Http::new(&job.settings)?,
            auth: Mutex::new(None),
            kube: Mutex::new(None),
            kube_cache: Mutex::new(None),
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
    pub(crate) async fn kube(
        &self,
        job: &Job,
        scope: &Scope,
        cancel: &CancellationToken,
    ) -> Result<CheckResult, Error> {
        let mut cache = scope.kube_cache.lock().await;
        if let Some(result) = cache.as_ref().filter(|r| {
            r.complete()
                && r.revision == job.revision
                && (chrono::Utc::now() - r.finished_at).num_seconds() < 30
        }) {
            let mut result = result.clone();
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
        *cache = Some(result.clone());
        Ok(result)
    }
}

#[async_trait::async_trait]
impl Collector for Router {
    async fn collect(&self, job: &Job, cancel: CancellationToken) -> CheckResult {
        tokio::select! {
            _ = cancel.cancelled() => CheckResult::failure(job.target.name.clone(), job.check, job.revision.clone(), Coverage::Cancelled),
            result = self.execute(job, &cancel) => match result {
                Ok(result) => result,
                Err(error) => CheckResult::failure(job.target.name.clone(), job.check, job.revision.clone(), error.coverage()),
            }
        }
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

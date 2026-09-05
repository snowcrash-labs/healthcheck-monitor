//! Shared target clients and credential recovery without interactive collection.
use crate::auth::Auth;
use monitor_core::{
    config::{resolve::Job, types::Config},
    model::*,
    scheduler::Collector,
};
use monitor_integrations::{
    endpoint, github,
    kubernetes::Kubernetes,
    nats::Nats,
    process::{Helper, Processes},
    projection::{observation, operation},
    transport::{Error, Http},
};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

struct Scope {
    http: Http,
    auth: Mutex<Option<Arc<Auth>>>,
    kube: Mutex<Option<Kubernetes>>,
    kube_cache: Mutex<Option<CheckResult>>,
    nats: Mutex<Option<Nats>>,
}
pub struct Router {
    config: RwLock<Config>,
    scopes: RwLock<BTreeMap<String, Arc<Scope>>>,
    processes: Processes,
}
impl Router {
    pub fn new(config: Config, subprocesses: usize) -> Self {
        Self {
            config: RwLock::new(config),
            scopes: RwLock::new(BTreeMap::new()),
            processes: Processes::new(subprocesses),
        }
    }
    pub async fn reload(&self, config: Config) {
        *self.config.write().await = config;
        self.scopes.write().await.clear();
    }
    async fn scope(&self, job: &Job) -> Result<Arc<Scope>, Error> {
        if let Some(scope) = self.scopes.read().await.get(&job.target.name) {
            return Ok(scope.clone());
        }
        let mut scopes = self.scopes.write().await;
        if let Some(scope) = scopes.get(&job.target.name) {
            return Ok(scope.clone());
        }
        if scopes.len() >= 128 {
            return Err(Error::Limit);
        }
        let scope = Arc::new(Scope {
            http: Http::new(&job.settings)?,
            auth: Mutex::new(None),
            kube: Mutex::new(None),
            kube_cache: Mutex::new(None),
            nats: Mutex::new(None),
        });
        scopes.insert(job.target.name.clone(), scope.clone());
        Ok(scope)
    }
    async fn auth(&self, job: &Job, scope: &Scope) -> Result<Arc<Auth>, Error> {
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
            ),
        )
        .await
        .map_err(|_| Error::Timeout)??;
        let loaded = Arc::new(loaded);
        *auth = Some(loaded.clone());
        Ok(loaded)
    }
    async fn kube(
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
    async fn execute(&self, job: &Job, cancel: &CancellationToken) -> Result<CheckResult, Error> {
        let scope = self.scope(job).await?;
        if job.check == Check::Kubernetes
            || job.target.provider == Provider::Kubernetes
                && job.check != Check::Edge
                && job.check != Check::Preflight
        {
            return self.kube(job, &scope, cancel).await;
        }
        if job.check == Check::Edge {
            let mut result = base(job);
            for endpoint in &job.target.endpoints {
                result
                    .observations
                    .push(endpoint::probe(&scope.http, job, endpoint, cancel).await);
            }
            result.operations.push(operation(
                "endpoints",
                if result.observations.is_empty() {
                    Err(&Error::Unavailable)
                } else {
                    Ok(result.observations.len())
                },
                1,
                true,
            ));
            result.finished_at = chrono::Utc::now();
            return Ok(result);
        }
        if job.target.provider == Provider::Github || job.check == Check::Github {
            let config = self.config.read().await;
            let env = job
                .target
                .credential
                .as_ref()
                .and_then(|k| config.credentials.get(k))
                .and_then(|p| p.token_env.as_deref())
                .unwrap_or("GH_TOKEN");
            let token = match std::env::var(env) {
                Ok(token) => token,
                Err(_) => {
                    let output = self
                        .processes
                        .run(
                            Helper::GithubToken,
                            16384,
                            job.settings.attempt_timeout.duration(),
                            cancel,
                        )
                        .await?;
                    String::from_utf8(output.stdout)
                        .map_err(|_| Error::Authentication)?
                        .trim()
                        .to_string()
                }
            };
            drop(config);
            return Ok(github::collect(&scope.http, job, &token, cancel).await);
        }
        if job.target.provider == Provider::Nats {
            let mut nats = scope.nats.lock().await;
            if nats.is_none() {
                let url = job.target.nats_url.as_ref().ok_or(Error::Authentication)?;
                *nats = Some(
                    tokio::time::timeout(
                        job.settings.connect_timeout.duration(),
                        Nats::connect(url.as_str()),
                    )
                    .await
                    .map_err(|_| Error::Timeout)??,
                );
            }
            let nats = nats.as_ref().ok_or(Error::Authentication)?;
            let observations = tokio::time::timeout(
                job.settings.operation_timeout.duration(),
                nats.collect(job, cancel),
            )
            .await
            .map_err(|_| Error::Timeout)??;
            let mut result = base(job);
            result
                .operations
                .push(operation("nats-streams", Ok(observations.len()), 1, true));
            result.observations = observations;
            return Ok(result);
        }
        if job.check == Check::Preflight
            && matches!(job.target.provider, Provider::Edge | Provider::Kubernetes)
        {
            let mut result = base(job);
            if job.target.provider == Provider::Kubernetes {
                let _ = self.kube(job, &scope, cancel).await?;
            }
            result
                .operations
                .push(operation("configured-scope", Ok(1), 1, true));
            result.observations.push(observation(
                job,
                "configured-scope",
                "target",
                Data::Identity {
                    scope: job.target.scope.clone(),
                },
            ));
            return Ok(result);
        }
        let auth = self.auth(job, &scope).await?;
        Ok(match job.target.provider {
            Provider::Gcp => crate::gcp::collect(&scope.http, &auth, job, cancel).await,
            Provider::Aws => crate::aws::collect(&scope.http, &auth, job, cancel).await,
            Provider::Azure => crate::azure::collect(&scope.http, &auth, job, cancel).await,
            _ => CheckResult::failure(
                job.target.name.clone(),
                job.check,
                job.revision.clone(),
                Coverage::Unsupported,
            ),
        })
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
fn base(job: &Job) -> CheckResult {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    result.operations.clear();
    result
}

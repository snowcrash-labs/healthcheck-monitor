//! Shared normalized GitHub metadata never caches credential material.
use crate::router::{Cached, Router, Scope};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{github, process::Helper, transport::Error};
use tokio_util::sync::CancellationToken;
impl Router {
    pub(crate) async fn github(
        &self,
        job: &Job,
        scope: &Scope,
        cancel: &CancellationToken,
    ) -> Result<CheckResult, Error> {
        let mut cache = scope.github_cache.lock().await;
        if job.check != Check::Preflight
            && let Some(cached) = cache.as_mut().filter(|cached| {
                cached.result.revision == job.revision
                    && !cached.consumers.contains(&job.check)
                    && (chrono::Utc::now() - cached.result.finished_at).num_seconds()
                        < job.settings.interval.0 as i64
            })
        {
            cached.consumers.insert(job.check);
            let mut result = cached.result.clone();
            result.check = job.check;
            return Ok(result);
        }
        if job.check != Check::Preflight {
            *cache = None;
        }
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
        let result = github::collect(&scope.http, job, &token, cancel).await;
        if job.check != Check::Preflight && result.complete() {
            *cache = None;
            if let Ok(bytes) = u32::try_from(monitor_core::bounds::result_bytes(&result))
                && let Ok(permit) = self.cache_bytes.clone().try_acquire_many_owned(bytes)
            {
                *cache = Some(Cached {
                    consumers: std::collections::BTreeSet::from([job.check]),
                    result: result.clone(),
                    _bytes: permit,
                });
            }
        }
        Ok(result)
    }
}

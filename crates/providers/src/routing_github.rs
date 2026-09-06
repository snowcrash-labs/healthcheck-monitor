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
        if job.check == Check::Preflight {
            let (token, expected) = self.github_token(job, cancel).await?;
            return Ok(
                github::collect(&scope.http, job, &token, expected.as_deref(), cancel).await,
            );
        }
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
        let (token, expected) = self.github_token(job, cancel).await?;
        let result = github::collect(&scope.http, job, &token, expected.as_deref(), cancel).await;
        if result.complete() {
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
    async fn github_token(
        &self,
        job: &Job,
        cancel: &CancellationToken,
    ) -> Result<(String, Option<String>), Error> {
        let profile = {
            let config = self.config.read().await;
            job.target
                .github_credential
                .as_ref()
                .or_else(|| {
                    (job.target.provider == Provider::Github)
                        .then_some(job.target.credential.as_ref())
                        .flatten()
                })
                .and_then(|name| config.credentials.get(name))
                .or_else(|| {
                    config
                        .credentials
                        .values()
                        .find(|profile| profile.provider == Provider::Github)
                })
                .cloned()
        };
        let expected = profile
            .as_ref()
            .and_then(|profile| profile.expected_identity.clone());
        if let Some(path) = profile
            .as_ref()
            .and_then(|profile| profile.credential_file.as_ref())
        {
            let token =
                token_file::read(path, job.settings.attempt_timeout.duration(), cancel).await?;
            return Ok((token, expected));
        }
        let env = profile
            .as_ref()
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
        Ok((token, expected))
    }
}

#[path = "github_token_file.rs"]
mod token_file;

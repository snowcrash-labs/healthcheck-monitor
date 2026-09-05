//! Route selected checks through shared clients and observations.
use crate::router::{Router, base};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{
    endpoint, github,
    nats::Nats,
    process::Helper,
    projection::{observation, operation},
    transport::Error,
};
use tokio_util::sync::CancellationToken;
impl Router {
    pub(crate) async fn execute(
        &self,
        job: &Job,
        cancel: &CancellationToken,
    ) -> Result<CheckResult, Error> {
        let scope = self.scope(job).await?;
        if job.check == Check::Queues && job.target.context.is_some() {
            let snapshot = self.kube(job, &scope, cancel).await?;
            let kube = scope
                .kube
                .lock()
                .await
                .as_ref()
                .ok_or(Error::Authentication)?
                .clone();
            let mut result =
                monitor_integrations::queues::collect(&kube, &snapshot, job, cancel).await;
            if let (Some(context), Some(fallback)) =
                (&job.target.context, &job.target.nats_fallback)
            {
                let output = self
                    .processes
                    .run(
                        Helper::NatsReport {
                            context: context.clone(),
                            fallback: fallback.clone(),
                        },
                        job.settings.response_bytes,
                        job.settings.operation_timeout.duration(),
                        cancel,
                    )
                    .await;
                let parsed = output
                    .and_then(|o| String::from_utf8(o.stdout).map_err(|_| Error::Malformed))
                    .and_then(|text| {
                        monitor_integrations::nats::parse_report(&text, job.settings.max_series)
                    });
                match parsed {
                    Ok(rows) => {
                        result
                            .operations
                            .push(operation("nats-streams", Ok(rows.len()), 1, true));
                        for (name, messages, bytes) in rows {
                            for (metric, value) in
                                [("stored-messages", messages), ("stored-bytes", bytes)]
                            {
                                result.observations.push(observation(
                                    job,
                                    "nats-streams",
                                    &format!("{name}/{metric}"),
                                    Data::Metric {
                                        name: metric.into(),
                                        value: value as f64,
                                        capacity: None,
                                        warning: None,
                                        error: None,
                                        window_seconds: 0,
                                    },
                                ));
                            }
                        }
                    }
                    Err(error) => {
                        result
                            .operations
                            .push(operation("nats-streams", Err(&error), 1, true))
                    }
                }
            }
            return Ok(result);
        }
        if job.check == Check::Kubernetes
            || job.target.provider == Provider::Kubernetes
                && job.check != Check::Edge
                && job.check != Check::Preflight
        {
            return self.kube(job, &scope, cancel).await;
        }
        if job.check == Check::Edge {
            let mut result = base(job);
            let mut endpoints = job.target.endpoints.clone();
            if job.target.context.is_some() {
                match self.kube(job, &scope, cancel).await {
                    Ok(snapshot) => {
                        for obs in snapshot.observations {
                            if let Data::AdvertisedEndpoint { url } = obs.data
                                && let Ok(url) = url::Url::parse(&url)
                                && endpoints.len() < 256
                                && !endpoints.iter().any(|e| e.url == url)
                            {
                                endpoints.push(monitor_core::config::types::Endpoint {
                                    name: obs.resource,
                                    url,
                                    accepted: vec![],
                                });
                            }
                        }
                        result.operations.extend(snapshot.operations);
                    }
                    Err(error) => result.operations.push(operation(
                        "endpoint-discovery",
                        Err(&error),
                        0,
                        true,
                    )),
                }
            }
            for endpoint in &endpoints {
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
        if job.check == Check::Discovery {
            let roots = self.config.read().await.discovery.clone();
            return Ok(crate::discovery::collect(&scope.http, &auth, job, &roots, cancel).await);
        }
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

//! Route selected checks through shared clients and observations.
use crate::router::{Router, base};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{
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
        if job.check == Check::Flows {
            let mut result = base(job);
            result
                .operations
                .push(operation("flow-inputs", Err(&Error::Missing), 0, true));
            return Ok(result);
        }
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
            return self.edge(job, &scope, cancel).await;
        }
        if job.target.provider == Provider::Github || job.check == Check::Github {
            return self.github(job, &scope, cancel).await;
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
                let observed = self.kube(job, &scope, cancel).await?;
                if let Some(failed) = observed.operations.iter().find(|operation| {
                    operation.required && operation.coverage != Coverage::Complete
                }) {
                    return Ok(CheckResult::failure(
                        job.target.name.clone(),
                        job.check,
                        job.revision.clone(),
                        failed.coverage,
                    ));
                }
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
        if job.check == Check::Slo && !job.target.slo_goals.is_empty() {
            let mut metrics = job.clone();
            metrics
                .target
                .metrics
                .retain(|query| job.target.slo_goals.contains_key(&query.name));
            let source = crate::common::NativeSource {
                http: &scope.http,
                auth: &auth,
                cache: Some(&scope.inventory),
                dedupe: None,
            };
            let mut result = match job.target.provider {
                Provider::Gcp => crate::metrics::gcp(&scope.http, &auth, &metrics, cancel).await,
                Provider::Aws => crate::aws_metrics::aws(&auth, &metrics, cancel).await,
                Provider::Azure => {
                    crate::azure_metrics::collect_from(&source, &metrics, cancel).await
                }
                _ => CheckResult::failure(
                    job.target.name.clone(),
                    job.check,
                    job.revision.clone(),
                    Coverage::Unsupported,
                ),
            };
            crate::slo_projection::configured(job, &mut result);
            return Ok(result);
        }
        if job.check == Check::Logs {
            let dedupe = self.log_dedupe(job, &scope).await;
            let source = crate::common::NativeSource {
                http: &scope.http,
                auth: &auth,
                cache: Some(&scope.inventory),
                dedupe: dedupe.as_deref(),
            };
            return Ok(match job.target.provider {
                Provider::Gcp => crate::gcp_logs::collect_from(&source, job, cancel).await,
                Provider::Aws => crate::aws_logs::collect_from(&source, job, cancel).await,
                Provider::Azure => crate::azure_logs::collect_from(&source, job, cancel).await,
                _ => CheckResult::failure(
                    job.target.name.clone(),
                    job.check,
                    job.revision.clone(),
                    Coverage::Unsupported,
                ),
            });
        }
        if job.check == Check::Discovery {
            let roots = self.config.read().await.discovery.clone();
            return Ok(crate::discovery::collect(&scope.http, &auth, job, &roots, cancel).await);
        }
        Ok(match job.target.provider {
            Provider::Gcp => {
                crate::gcp::collect(&scope.http, &auth, job, cancel, &scope.inventory).await
            }
            Provider::Aws => {
                crate::aws::collect(&scope.http, &auth, job, cancel, &scope.inventory).await
            }
            Provider::Azure => {
                crate::azure::collect(&scope.http, &auth, job, cancel, &scope.inventory).await
            }
            _ => CheckResult::failure(
                job.target.name.clone(),
                job.check,
                job.revision.clone(),
                Coverage::Unsupported,
            ),
        })
    }
}

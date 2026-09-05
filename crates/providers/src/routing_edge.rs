//! Public endpoint discovery reuses normalized control-plane and Kubernetes reads.
use crate::router::{Router, Scope, base};
use futures::{StreamExt, stream};
use monitor_core::{
    config::{resolve::Job, types::Endpoint},
    model::*,
};
use monitor_integrations::{endpoint, projection::operation, transport::Error};
use tokio_util::sync::CancellationToken;
impl Router {
    pub(crate) async fn edge(
        &self,
        job: &Job,
        scope: &Scope,
        cancel: &CancellationToken,
    ) -> Result<CheckResult, Error> {
        let mut result = base(job);
        let mut endpoints = job.target.endpoints.clone();
        if job.target.context.is_some() {
            match self.kube(job, scope, cancel).await {
                Ok(snapshot) => append(&mut endpoints, &mut result, snapshot),
                Err(error) => result.operations.push(operation(
                    "kubernetes-endpoint-discovery",
                    Err(&error),
                    0,
                    true,
                )),
            }
        }
        if matches!(
            job.target.provider,
            Provider::Gcp | Provider::Aws | Provider::Azure
        ) {
            match self.auth(job, scope).await {
                Ok(auth) => {
                    let snapshot = match job.target.provider {
                        Provider::Gcp => {
                            crate::gcp::collect(&scope.http, &auth, job, cancel, &scope.inventory)
                                .await
                        }
                        Provider::Aws => {
                            crate::aws::collect(&scope.http, &auth, job, cancel, &scope.inventory)
                                .await
                        }
                        Provider::Azure => {
                            crate::azure::collect(&scope.http, &auth, job, cancel, &scope.inventory)
                                .await
                        }
                        _ => base(job),
                    };
                    append(&mut endpoints, &mut result, snapshot);
                }
                Err(error) => result.operations.push(operation(
                    "cloud-endpoint-discovery",
                    Err(&error),
                    0,
                    true,
                )),
            }
        }
        let mut probes = stream::iter(0..endpoints.len())
            .map(|index| endpoint::probe(&scope.http, job, &endpoints[index], cancel))
            .buffer_unordered(monitor_integrations::admission::width(&job.settings));
        while let Some(observation) = probes.next().await {
            result.observations.push(observation);
        }
        result.operations.push(operation(
            "endpoints",
            if result.observations.is_empty() {
                Err(&Error::Missing)
            } else {
                Ok(result.observations.len())
            },
            1,
            true,
        ));
        result.finished_at = chrono::Utc::now();
        Ok(result)
    }
}
fn append(endpoints: &mut Vec<Endpoint>, result: &mut CheckResult, snapshot: CheckResult) {
    for observation in snapshot.observations {
        if let Data::AdvertisedEndpoint { url } = observation.data
            && let Ok(url) = url::Url::parse(&url)
        {
            if endpoints.iter().any(|e| e.url == url) {
                continue;
            }
            if endpoints.len() >= 256 {
                result
                    .operations
                    .push(operation("endpoint-limit", Err(&Error::Limit), 0, true));
                break;
            }
            endpoints.push(Endpoint {
                name: observation.resource,
                url,
                accepted: vec![],
            });
        }
    }
    // Discovery depends on routing metadata; unrelated warning-event limits do not block probes.
    result
        .operations
        .extend(snapshot.operations.into_iter().filter(|op| {
            !matches!(
                op.id.as_str(),
                "events"
                    | "pods"
                    | "jobs"
                    | "cronjobs"
                    | "nodes"
                    | "replicasets"
                    | "deployments"
                    | "statefulsets"
                    | "daemonsets"
                    | "horizontalpodautoscalers"
                    | "scaledobjects"
                    | "externalsecrets"
            )
        }));
}

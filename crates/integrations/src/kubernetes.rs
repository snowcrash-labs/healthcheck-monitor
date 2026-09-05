//! Kubernetes metadata projection through bounded native kube transport.
use super::transport::Error;
use http_body_util::BodyExt;
use monitor_core::{config::resolve::Job, model::*};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct Kubernetes {
    session: std::sync::Arc<tokio::sync::Mutex<crate::kube_auth::Session>>,
}
impl Kubernetes {
    pub async fn new(
        context: Option<&str>,
        processes: crate::process::Processes,
    ) -> Result<Self, Error> {
        let config = if let Some(context) = context {
            kube::Config::from_kubeconfig(&kube::config::KubeConfigOptions {
                context: Some(context.into()),
                ..Default::default()
            })
            .await
            .map_err(|_| Error::Authentication)?
        } else {
            kube::Config::infer()
                .await
                .map_err(|_| Error::Authentication)?
        };
        Ok(Self {
            session: std::sync::Arc::new(tokio::sync::Mutex::new(crate::kube_auth::Session::new(
                config, processes,
            )?)),
        })
    }
    /// Raw native send avoids kube's unbounded error-body collection in request_stream.
    pub async fn json(
        &self,
        path: &str,
        job: &Job,
        cancel: &CancellationToken,
    ) -> Result<Value, Error> {
        let operation = async {
            for attempt in 0..job.settings.attempts {
                let result = tokio::time::timeout(
                    job.settings.attempt_timeout.duration(),
                    self.once(path, job, cancel),
                )
                .await
                .map_err(|_| Error::Timeout)
                .and_then(|result| result);
                match result {
                    Ok(value) => return Ok(value),
                    Err(error) if error.retryable() && attempt + 1 < job.settings.attempts => {
                        tokio::time::sleep(std::time::Duration::from_millis(200 * (1 << attempt)))
                            .await;
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(Error::Unavailable)
        };
        tokio::select! {
            _ = cancel.cancelled() => Err(Error::Cancelled),
            result = tokio::time::timeout(job.settings.operation_timeout.duration(), operation) => result.map_err(|_| Error::Timeout).and_then(|r| r),
        }
    }
    async fn once(
        &self,
        path: &str,
        job: &Job,
        cancel: &CancellationToken,
    ) -> Result<Value, Error> {
        let client = self.session.lock().await.client(job, cancel).await?;
        let _permit = crate::admission::acquire().await?;
        let request = http::Request::get(path)
            .body(kube::client::Body::empty())
            .map_err(|_| Error::Malformed)?;
        let response = client.send(request).await.map_err(|_| Error::Unavailable)?;
        super::transport::status(response.status())?;
        if response
            .headers()
            .get(http::header::CONTENT_LENGTH)
            .and_then(|length| length.to_str().ok())
            .and_then(|length| length.parse::<usize>().ok())
            .is_some_and(|length| length > job.settings.response_bytes)
        {
            return Err(Error::Limit);
        }
        let mut body = response.into_body();
        let mut bytes = Vec::new();
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|_| Error::Unavailable)?;
            if let Ok(chunk) = frame.into_data() {
                if chunk.len() > job.settings.response_bytes.saturating_sub(bytes.len()) {
                    return Err(Error::Limit);
                }
                bytes.extend_from_slice(&chunk);
            }
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::Malformed)
    }
    pub async fn collect(&self, job: &Job, cancel: &CancellationToken) -> CheckResult {
        crate::kube_collect::collect(self, job, cancel).await
    }
}
#[cfg(test)]
#[path = "kube_transport_tests.rs"]
mod tests;
pub(crate) fn required_kind(job: &Job, kind: &str) -> bool {
    let requested = &job.requested_checks;
    if requested.contains(&Check::Kubernetes)
        || requested.len() == 1 && requested.contains(&Check::Preflight)
        || job.target.provider == Provider::Kubernetes
            && requested
                .iter()
                .any(|check| matches!(check, Check::Inventory | Check::Managed))
    {
        return true;
    }
    let queues = requested.contains(&Check::Queues) || requested.contains(&Check::Flows);
    let releases = requested.contains(&Check::Releases);
    let edge = requested.contains(&Check::Edge);
    match kind {
        "pods" | "deployments" | "statefulsets" | "replicasets" | "daemonsets" => {
            queues || releases
        }
        "jobs" | "cronjobs" => releases,
        "horizontalpodautoscalers" | "scaledobjects" => queues,
        "ingresses" | "httproutes" | "mappings" | "certificates" => edge,
        _ => false,
    }
}
pub(crate) const KINDS: &[(&str, &str)] = &[
    ("nodes", "/api/v1/nodes"),
    ("pods", "/api/v1/pods"),
    ("deployments", "/apis/apps/v1/deployments"),
    ("statefulsets", "/apis/apps/v1/statefulsets"),
    ("replicasets", "/apis/apps/v1/replicasets"),
    ("daemonsets", "/apis/apps/v1/daemonsets"),
    ("jobs", "/apis/batch/v1/jobs"),
    ("cronjobs", "/apis/batch/v1/cronjobs"),
    (
        "horizontalpodautoscalers",
        "/apis/autoscaling/v2/horizontalpodautoscalers",
    ),
    ("scaledobjects", "/apis/keda.sh/v1alpha1/scaledobjects"),
    ("events", "/api/v1/events"),
    ("ingresses", "/apis/networking.k8s.io/v1/ingresses"),
    (
        "httproutes",
        "/apis/gateway.networking.k8s.io/v1/httproutes",
    ),
    ("mappings", "/apis/getambassador.io/v3alpha1/mappings"),
    ("certificates", "/apis/cert-manager.io/v1/certificates"),
    (
        "externalsecrets",
        "/apis/external-secrets.io/v1/externalsecrets",
    ),
];

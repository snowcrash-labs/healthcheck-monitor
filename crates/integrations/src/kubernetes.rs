//! Kubernetes metadata projection through bounded native kube transport.
use super::{
    projection::{operation, text},
    transport::Error,
};
use futures::AsyncReadExt;
use monitor_core::{config::resolve::Job, model::*};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct Kubernetes {
    client: kube::Client,
}
impl Kubernetes {
    pub async fn new(context: Option<&str>) -> Result<Self, Error> {
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
            client: kube::Client::try_from(config).map_err(|_| Error::Authentication)?,
        })
    }
    /// request_stream avoids kube's unbounded request_text accumulation.
    pub async fn json(
        &self,
        path: &str,
        job: &Job,
        cancel: &CancellationToken,
    ) -> Result<Value, Error> {
        let request = http::Request::get(path)
            .body(Vec::new())
            .map_err(|_| Error::Malformed)?;
        let operation = async {
            let stream = self
                .client
                .request_stream(request)
                .await
                .map_err(|e| match e {
                    kube::Error::Api(response) if response.code == 403 => Error::Denied,
                    kube::Error::Api(response) if response.code == 401 => Error::Authentication,
                    kube::Error::Api(response) if response.code == 404 => Error::Unavailable,
                    _ => Error::Unavailable,
                })?;
            let mut bytes = Vec::new();
            stream
                .take(job.settings.response_bytes as u64 + 1)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| Error::Unavailable)?;
            if bytes.len() > job.settings.response_bytes {
                return Err(Error::Limit);
            }
            serde_json::from_slice(&bytes).map_err(|_| Error::Malformed)
        };
        tokio::select! {
            _ = cancel.cancelled() => Err(Error::Cancelled),
            result = tokio::time::timeout(job.settings.operation_timeout.duration(), operation) => result.map_err(|_| Error::Timeout).and_then(|r| r),
        }
    }
    pub async fn collect(&self, job: &Job, cancel: &CancellationToken) -> CheckResult {
        let mut result = CheckResult::failure(
            job.target.name.clone(),
            job.check,
            job.revision.clone(),
            Coverage::Missing,
        );
        result.operations.clear();
        for (kind, api) in KINDS {
            if cancel.is_cancelled() {
                break;
            }
            let mut token = String::new();
            let mut count = 0;
            let mut outcome = Ok(0);
            let mut pages = 0;
            for page in 0..job.settings.max_pages {
                pages = page + 1;
                let query: String = url::form_urlencoded::Serializer::new(String::new())
                    .append_pair("limit", &job.settings.page_size.to_string())
                    .append_pair("continue", &token)
                    .finish();
                match self.json(&format!("{api}?{query}"), job, cancel).await {
                    Ok(payload) => {
                        let Some(items) = payload.get("items").and_then(Value::as_array) else {
                            outcome = Err(Error::Malformed);
                            break;
                        };
                        for item in items {
                            if result.observations.len() >= job.settings.max_assets {
                                outcome = Err(Error::Limit);
                                break;
                            }
                            let projected = super::kube_projection::project(job, kind, item);
                            count += projected.len();
                            result.observations.extend(projected);
                        }
                        if outcome.is_err() {
                            break;
                        }
                        outcome = Ok(count);
                        token = text(&payload, &["/metadata/continue"])
                            .unwrap_or("")
                            .to_string();
                        if token.is_empty() {
                            break;
                        }
                        if page + 1 == job.settings.max_pages {
                            outcome = Err(Error::Limit);
                        }
                    }
                    Err(error) => {
                        outcome = Err(error);
                        break;
                    }
                }
            }
            let required = !api.contains(".io/")
                || matches!(outcome, Err(Error::Denied | Error::Authentication));
            result.operations.push(operation(
                kind,
                outcome.as_ref().copied(),
                pages,
                required && job.settings.required,
            ));
        }
        result.finished_at = chrono::Utc::now();
        result
    }
}
const KINDS: &[(&str, &str)] = &[
    ("nodes", "/api/v1/nodes"),
    ("pods", "/api/v1/pods"),
    ("deployments", "/apis/apps/v1/deployments"),
    ("statefulsets", "/apis/apps/v1/statefulsets"),
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

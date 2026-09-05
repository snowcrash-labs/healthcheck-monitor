//! Per-API aggregation preserves unrelated successes when a scan fails.
use crate::auth::Auth;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::transport::{Error, Http};
use serde_json::Value;
use sha2::Digest;
use tokio_util::sync::CancellationToken;

pub use crate::endpoint::Endpoint;
pub async fn request(
    http: &Http,
    auth: &Auth,
    endpoint: &Endpoint,
    job: &Job,
    cancel: &CancellationToken,
) -> Result<Value, Error> {
    request_cached(http, auth, endpoint, job, cancel, None).await
}
async fn request_cached(
    http: &Http,
    auth: &Auth,
    endpoint: &Endpoint,
    job: &Job,
    cancel: &CancellationToken,
    cache: Option<&crate::inventory_cache::InventoryCache>,
) -> Result<Value, Error> {
    tokio::select! {
        _ = cancel.cancelled() => Err(Error::Cancelled),
        result = tokio::time::timeout(job.settings.operation_timeout.duration(), request_inner(http, auth, endpoint, job, cancel,cache)) => result.map_err(|_| Error::Timeout).and_then(|r| r),
    }
}
async fn request_inner(
    http: &Http,
    auth: &Auth,
    endpoint: &Endpoint,
    job: &Job,
    cancel: &CancellationToken,
    cache: Option<&crate::inventory_cache::InventoryCache>,
) -> Result<Value, Error> {
    if let Some(result) = crate::aws_native::request(auth, endpoint, job).await {
        return result;
    }
    if endpoint.aws.is_some() {
        return Err(Error::Authentication);
    }
    if endpoint.id.starts_with("acr-") {
        return crate::azure_registry_auth::request(
            http,
            auth,
            endpoint,
            job,
            cancel,
            cache.map(|cache| &cache.tokens),
        )
        .await;
    }
    if endpoint.id.starts_with("registry-manifest/") && endpoint.aws.is_none() {
        return crate::registry_transport::gcp(
            http,
            auth,
            endpoint,
            job,
            cancel,
            cache.map(|cache| &cache.tokens),
        )
        .await;
    }
    let builder = if let Some(body) = &endpoint.body {
        http.client().post(&endpoint.url).json(body)
    } else {
        http.client().get(&endpoint.url)
    };
    let builder = if endpoint.id.starts_with("kv-") {
        builder.bearer_auth(auth.vault_bearer().await?)
    } else {
        builder.bearer_auth(
            auth.bearer_for(
                endpoint
                    .url
                    .starts_with("https://api.loganalytics.azure.com/"),
            )
            .await?,
        )
    };
    let request = builder.build().map_err(|_| Error::Malformed)?;
    http.json(request, &job.settings, cancel).await
}
pub async fn collect(
    http: &Http,
    auth: &Auth,
    job: &Job,
    endpoints: Vec<Endpoint>,
    cancel: &CancellationToken,
) -> CheckResult {
    let source = NativeSource {
        dedupe: None,
        http,
        auth,
        cache: None,
    };
    collect_from(&source, job, endpoints, cancel).await
}

pub trait Source: Send + Sync {
    fn dedupe(&self) -> Option<&monitor_integrations::log_dedup::Dedupe> {
        None
    }
    fn cache(&self) -> Option<&crate::inventory_cache::InventoryCache> {
        None
    }
    fn request(
        &self,
        endpoint: &Endpoint,
        job: &Job,
        cancel: &CancellationToken,
    ) -> impl std::future::Future<Output = Result<Value, Error>> + Send;
}
pub(crate) fn cache_key(job: &Job, endpoint: &Endpoint) -> String {
    sha2::Sha256::digest(format!(
        "{}:{}:{}:{:?}",
        job.revision, endpoint.id, endpoint.url, endpoint.body
    ))
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect()
}

pub(crate) struct NativeSource<'a> {
    pub dedupe: Option<&'a monitor_integrations::log_dedup::Dedupe>,
    pub http: &'a Http,
    pub auth: &'a Auth,
    pub cache: Option<&'a crate::inventory_cache::InventoryCache>,
}
impl Source for NativeSource<'_> {
    fn dedupe(&self) -> Option<&monitor_integrations::log_dedup::Dedupe> {
        self.dedupe
    }
    fn cache(&self) -> Option<&crate::inventory_cache::InventoryCache> {
        self.cache
    }
    async fn request(
        &self,
        endpoint: &Endpoint,
        job: &Job,
        cancel: &CancellationToken,
    ) -> Result<Value, Error> {
        tokio::select! {
            _ = cancel.cancelled() => Err(Error::Cancelled),
            result = tokio::time::timeout(job.settings.operation_timeout.duration(), request_cached(self.http, self.auth, endpoint, job, cancel,self.cache)) => result.map_err(|_| Error::Timeout).and_then(|r| r),
        }
    }
}
pub async fn collect_from<S: Source>(
    source: &S,
    job: &Job,
    endpoints: Vec<Endpoint>,
    cancel: &CancellationToken,
) -> CheckResult {
    crate::scan_budget::run(
        job,
        crate::common_parallel::collect(source, job, endpoints, cancel),
    )
    .await
}
pub async fn collect_cached(
    http: &Http,
    auth: &Auth,
    job: &Job,
    endpoints: Vec<Endpoint>,
    cancel: &CancellationToken,
    cache: &crate::inventory_cache::InventoryCache,
) -> CheckResult {
    collect_from(
        &NativeSource {
            dedupe: None,
            http,
            auth,
            cache: Some(cache),
        },
        job,
        endpoints,
        cancel,
    )
    .await
}

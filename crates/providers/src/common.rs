//! Per-API aggregation preserves unrelated successes when a scan fails.
use crate::auth::Auth;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{
    projection::operation,
    transport::{Error, Http},
};
use serde_json::Value;
use sha2::Digest;
use std::collections::{BTreeSet, VecDeque};
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
    let query = endpoint
        .aws
        .as_ref()
        .is_some_and(|(_, _, target)| target.starts_with("query:"));
    let mut builder = if query {
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        if let Some(fields) = endpoint.body.as_ref().and_then(Value::as_object) {
            for (name, value) in fields {
                if let Some(value) = value.as_str() {
                    form.append_pair(name, value);
                }
            }
        }
        http.client()
            .post(&endpoint.url)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form.finish())
    } else if let Some(body) = &endpoint.body {
        http.client().post(&endpoint.url).json(body)
    } else {
        http.client().get(&endpoint.url)
    };
    if let Some((_, _, target)) = &endpoint.aws {
        if !query && !target.is_empty() {
            builder = builder
                .header("x-amz-target", target)
                .header("content-type", "application/x-amz-json-1.1");
        }
    } else {
        builder = if endpoint.id.starts_with("kv-") {
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
    }
    let mut request = builder.build().map_err(|_| Error::Malformed)?;
    if let Some((service, region, _)) = &endpoint.aws {
        auth.sign(&mut request, service, region).await?;
    }
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
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    result.operations.clear();
    let mut initial = endpoints.into_iter();
    let mut pending = VecDeque::new();
    let mut queued_bytes = 0usize;
    let queue_bytes = job.settings.memory_bytes / 4 / job.settings.concurrency.max(1);
    let mut budget = monitor_core::collection_budget::Limit::new(&job.settings);
    let mut visited = BTreeSet::new();
    loop {
        if budget.exhausted() {
            if !pending.is_empty() || initial.len() > 0 {
                budget.mark_limited();
            }
            break;
        }
        let endpoint = if let Some(endpoint) = pending.pop_front() {
            queued_bytes = queued_bytes.saturating_sub(Endpoint::bytes(&endpoint));
            endpoint
        } else if let Some(endpoint) = initial.next() {
            endpoint
        } else {
            break;
        };
        let key = cache_key(job, &endpoint);
        if !visited.insert(key.clone()) {
            continue;
        }
        if visited.len() > job.settings.max_assets {
            budget.mark_limited();
            break;
        }
        if cancel.is_cancelled() {
            budget.operations(
                &mut result.operations,
                [operation(
                    &endpoint.id,
                    Err(&Error::Cancelled),
                    0,
                    job.settings.required,
                )],
            );
            continue;
        }
        let mut fetched = if let Some(cache) = source.cache() {
            cache.load(source, job, endpoint, cancel, key).await
        } else {
            crate::endpoint_scan::fetch(source, job, endpoint, cancel).await
        };
        for followup in fetched.followups {
            let bytes = followup.bytes();
            if pending.len() >= job.settings.ready_queue
                || bytes > queue_bytes.saturating_sub(queued_bytes)
            {
                for op in &mut fetched.result.operations {
                    op.coverage = Coverage::Truncated;
                }
                break;
            }
            queued_bytes += bytes;
            pending.push_back(followup);
        }
        for op in &mut fetched.result.operations {
            op.required = job.settings.required;
        }
        budget.merge(&mut result, fetched.result);
    }
    budget.finish(&mut result, job.settings.required);
    if result.operations.is_empty() {
        result.operations.push(operation(
            "not-configured",
            Err(&Error::Missing),
            0,
            job.settings.required,
        ));
    }
    result.finished_at = chrono::Utc::now();
    result
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

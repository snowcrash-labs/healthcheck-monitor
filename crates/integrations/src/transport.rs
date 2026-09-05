//! Reused rustls transport with bounded decoding and a closed read-only POST policy.
use monitor_core::{config::settings::Settings, model::Coverage};
use reqwest::{Client, Request, Response, StatusCode};
use serde_json::Value;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("provider denied access")]
    Denied,
    #[error("credentials unavailable or expired")]
    Authentication,
    #[error("service unavailable")]
    Unavailable,
    #[error("request deadline expired")]
    Timeout,
    #[error("provider throttled request")]
    Throttled,
    #[error("malformed provider response")]
    Malformed,
    #[error("response or pagination limit reached")]
    Limit,
    #[error("operation cancelled")]
    Cancelled,
    #[error("operation is outside read-only policy")]
    Forbidden,
}
impl Error {
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Unavailable | Self::Throttled | Self::Timeout)
    }
    pub fn coverage(&self) -> Coverage {
        match self {
            Self::Denied | Self::Forbidden => Coverage::Denied,
            Self::Authentication => Coverage::Unauthenticated,
            Self::Unavailable | Self::Throttled => Coverage::Unavailable,
            Self::Timeout => Coverage::Timeout,
            Self::Malformed => Coverage::Malformed,
            Self::Limit => Coverage::Truncated,
            Self::Cancelled => Coverage::Cancelled,
        }
    }
}
#[derive(Clone)]
pub struct Http {
    client: Client,
}
impl Http {
    pub fn new(settings: &Settings) -> Result<Self, Error> {
        let client = Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(settings.connect_timeout.duration())
            .timeout(settings.attempt_timeout.duration())
            .pool_max_idle_per_host(settings.scope_concurrency)
            .pool_idle_timeout(Duration::from_secs(60))
            .user_agent("Soundpatrol-healthcheck-monitor/0.1")
            .build()
            .map_err(|_| Error::Unavailable)?;
        Ok(Self { client })
    }
    pub fn client(&self) -> &Client {
        &self.client
    }
    /// The caller authorizes credentials, but cannot widen the wire-level operation policy.
    pub async fn json(
        &self,
        request: Request,
        settings: &Settings,
        cancel: &CancellationToken,
    ) -> Result<Value, Error> {
        if !allowed(&request) {
            return Err(Error::Forbidden);
        }
        let deadline = tokio::time::Instant::now() + settings.operation_timeout.duration();
        for attempt in 0..settings.attempts {
            let request = request.try_clone().ok_or(Error::Forbidden)?;
            let outcome = tokio::select! {
                _ = cancel.cancelled() => return Err(Error::Cancelled),
                result = tokio::time::timeout_at(deadline, self.once(request, settings)) => result.map_err(|_| Error::Timeout).and_then(|r| r),
            };
            match outcome {
                Ok(value) => return Ok(value),
                Err(error) if error.retryable() && attempt + 1 < settings.attempts => {
                    let delay =
                        Duration::from_millis(200 * (1 << attempt) + (attempt as u64 * 73) % 150);
                    tokio::select! { _ = cancel.cancelled() => return Err(Error::Cancelled), _ = tokio::time::sleep_until((tokio::time::Instant::now() + delay).min(deadline)) => {} }
                }
                Err(error) => return Err(error),
            }
        }
        Err(Error::Unavailable)
    }
    async fn once(&self, request: Request, settings: &Settings) -> Result<Value, Error> {
        let response = self.client.execute(request).await.map_err(|error| {
            if error.is_timeout() {
                Error::Timeout
            } else {
                Error::Unavailable
            }
        })?;
        status(response.status())?;
        let bytes = bounded(response, settings.response_bytes).await?;
        if bytes.first() == Some(&b'<') {
            super::xml::decode(&bytes)
        } else {
            serde_json::from_slice(&bytes).map_err(|_| Error::Malformed)
        }
    }
}
pub fn status(status: StatusCode) -> Result<(), Error> {
    match status.as_u16() {
        200..=299 => Ok(()),
        401 => Err(Error::Authentication),
        403 => Err(Error::Denied),
        429 => Err(Error::Throttled),
        404 | 410 | 501 => Err(Error::Unavailable),
        500..=599 => Err(Error::Unavailable),
        _ => Err(Error::Malformed),
    }
}
pub async fn bounded(mut response: Response, limit: usize) -> Result<Vec<u8>, Error> {
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err(Error::Limit);
    }
    let mut bytes = Vec::with_capacity(limit.min(65536));
    while let Some(chunk) = response.chunk().await.map_err(|_| Error::Unavailable)? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(Error::Limit);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
/// Deny secret retrieval, message consumption, object bodies, and arbitrary POST calls.
pub fn allowed(request: &Request) -> bool {
    let path = request.url().path().to_ascii_lowercase();
    if request.url().scheme() != "https"
        || path.contains(":access")
        || path.contains(":decrypt")
        || path.contains("/exec")
        || path.contains("/attach")
        || path.contains("/proxy")
        || path.contains(":pull")
        || path.contains(":acknowledge")
        || path.contains("/listkeys")
        || path.contains("/listsecrets")
    {
        return false;
    }
    match *request.method() {
        reqwest::Method::GET | reqwest::Method::HEAD => {
            let query = request.url().query().unwrap_or("").to_ascii_lowercase();
            !query.contains("alt=media")
                && !path.contains("/objects/")
                && !path.contains("/secrets/")
                || path.ends_with("/versions")
        }
        reqwest::Method::POST => {
            if request
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                == Some("application/x-www-form-urlencoded")
            {
                let body = request.body().and_then(|b| b.as_bytes()).unwrap_or(&[]);
                let action = url::form_urlencoded::parse(body)
                    .find(|(k, _)| k == "Action")
                    .map(|(_, v)| v.into_owned())
                    .unwrap_or_default();
                return [
                    "DescribeInstances",
                    "DescribeInstanceStatus",
                    "DescribeVolumes",
                    "DescribeAutoScalingGroups",
                    "DescribeLoadBalancers",
                    "DescribeTargetGroups",
                    "DescribeTargetHealth",
                    "DescribeDBInstances",
                    "DescribeDBClusters",
                    "DescribeCacheClusters",
                    "DescribeReplicationGroups",
                    "ListTopics",
                    "GetTopicAttributes",
                    "DescribeRegions",
                    "DescribeAccountAttributes",
                ]
                .contains(&action.as_str());
            }
            if path == "/v2/entries:list"
                || path.ends_with("/providers/microsoft.resourcegraph/resources")
                || path.ends_with("/gethealth")
            {
                return true;
            }
            let target = request
                .headers()
                .get("x-amz-target")
                .and_then(|s| s.to_str().ok())
                .unwrap_or("");
            let action = target.rsplit('.').next().unwrap_or("");
            const READS: &[&str] = &[
                "DescribeTable",
                "ListTables",
                "DescribeClusters",
                "DescribeServices",
                "ListClusters",
                "ListServices",
                "ListFunctions",
                "DescribeLogGroups",
                "FilterLogEvents",
                "ListQueues",
                "GetQueueAttributes",
                "ListRules",
                "ListEventBuses",
                "ListEventSourceMappings",
                "DescribeAlarms",
                "GetMetricData",
                "ListAccounts",
                "DescribeOrganization",
                "DescribeRepositories",
                "ListImages",
                "DescribeImages",
                "ListBuilds",
                "BatchGetBuilds",
                "ListPipelines",
                "GetPipelineState",
                "ListBackupVaults",
                "ListRecoveryPointsByBackupVault",
                "ListKeys",
                "DescribeKey",
                "ListSecrets",
                "ListServiceQuotas",
                "ListServices",
                "DescribeEvents",
                "DescribeAffectedEntities",
                "DescribeSubscriptionFilters",
            ];
            READS.contains(&action)
        }
        _ => false,
    }
}

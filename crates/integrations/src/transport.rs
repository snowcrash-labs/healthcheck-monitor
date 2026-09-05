//! Reused rustls transport with bounded decoding and a closed read-only POST policy.
use monitor_core::{config::settings::Settings, model::Coverage};
use reqwest::{Client, Request, Response, StatusCode};
use serde_json::Value;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("required telemetry was not returned")]
    Missing,
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
            Self::Missing => Coverage::Missing,
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
        mut request: Request,
        settings: &Settings,
        cancel: &CancellationToken,
    ) -> Result<Value, Error> {
        if !allowed(&request) {
            return Err(Error::Forbidden);
        }
        *request.timeout_mut() = Some(settings.attempt_timeout.duration());
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
        let registry = request
            .url()
            .host_str()
            .is_some_and(|host| host.ends_with(".azurecr.io"));
        let response = self.client.execute(request).await.map_err(|error| {
            if error.is_timeout() {
                Error::Timeout
            } else {
                Error::Unavailable
            }
        })?;
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            let delay = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .map(Duration::from_secs);
            if let Some(delay) = delay {
                tokio::time::sleep(delay).await;
            }
        }
        if !response.status().is_success() {
            let code = response.status();
            let bytes = bounded(response, settings.response_bytes.min(65536)).await?;
            return Err(response_error(code, &bytes));
        }
        let next = if registry {
            response
                .headers()
                .get("link")
                .and_then(|header| header.to_str().ok())
                .filter(|value| value.len() <= 4096)
                .and_then(|value| {
                    value
                        .split(',')
                        .find(|part| part.contains("rel=\"next\"") || part.contains("rel=next"))
                })
                .and_then(|part| part.split(';').next())
                .map(|link| {
                    link.trim()
                        .trim_start_matches('<')
                        .trim_end_matches('>')
                        .to_owned()
                })
        } else {
            None
        };
        let bytes = bounded(response, settings.response_bytes).await?;
        if bytes.first() == Some(&b'<') {
            super::xml::decode(&bytes)
        } else {
            let mut value: Value = serde_json::from_slice(&bytes).map_err(|_| Error::Malformed)?;
            if let Some(next) = next
                && let Some(object) = value.as_object_mut()
            {
                object.insert("_monitor_next".into(), Value::String(next));
            }
            Ok(value)
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
/// Only a fixed error vocabulary crosses the provider boundary; response messages are discarded.
pub fn response_error(status_code: StatusCode, bytes: &[u8]) -> Error {
    let payload = if bytes.first() == Some(&b'<') {
        super::xml::decode(bytes).ok()
    } else {
        serde_json::from_slice::<Value>(bytes).ok()
    };
    let code = payload
        .as_ref()
        .and_then(|value| {
            super::projection::text(
                value,
                &[
                    "/__type",
                    "/code",
                    "/error/code",
                    "/error/status",
                    "/Error/Code",
                    "/Code",
                ],
            )
        })
        .and_then(|code| code.rsplit('#').next())
        .unwrap_or("")
        .to_ascii_lowercase();
    match code.as_str() {
        "expiredtoken"
        | "expiredtokenexception"
        | "invalidclienttokenid"
        | "unrecognizedclientexception"
        | "unauthenticated"
        | "authenticationfailed"
        | "invalidauthenticationtoken" => Error::Authentication,
        "accessdenied" | "accessdeniedexception" | "authorizationfailed" | "permission_denied" => {
            Error::Denied
        }
        "throttling"
        | "throttlingexception"
        | "toomanyrequestsexception"
        | "resource_exhausted" => Error::Throttled,
        "subscriptionrequiredexception"
        | "subscriptionnotenabled"
        | "unsupportedoperation"
        | "featurenotsupportedexception" => Error::Unavailable,
        _ => status(status_code).err().unwrap_or(Error::Malformed),
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
pub use crate::read_policy::allowed;

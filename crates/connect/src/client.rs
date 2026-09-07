//! One pooled HTTP client enforces fixed destinations, response bounds, and bounded retries.
use crate::{auth::Auth, config::Config, credentials::CredentialsStore, error::Error};
use serde::{Serialize, de::DeserializeOwned};
use std::{sync::Arc, time::Duration};

#[derive(Clone)]
pub struct Client {
    pub config: Arc<Config>,
    pub http: reqwest::Client,
    pub store: CredentialsStore,
    pub auth: Arc<tokio::sync::Mutex<Auth>>,
    requests: Arc<tokio::sync::Semaphore>,
}
impl Client {
    pub fn new(config: Config) -> Result<Self, Error> {
        config.validate()?;
        let http = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(4)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(30))
            .user_agent("healthcheck-connect/0.1")
            .build()
            .map_err(|_| Error::Configuration)?;
        let store = CredentialsStore::new(&config);
        Ok(Self {
            config: Arc::new(config),
            http,
            store,
            auth: Arc::new(tokio::sync::Mutex::new(Auth::default())),
            requests: Arc::new(tokio::sync::Semaphore::new(4)),
        })
    }
    pub async fn get<Q: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        endpoint: &str,
        query: &Q,
    ) -> Result<T, Error> {
        if ![
            "scopes",
            "summary",
            "findings",
            "diagnostics",
            "resource",
            "checks",
            "deployment",
        ]
        .contains(&endpoint)
        {
            return Err(Error::Query);
        }
        let _permit = self
            .requests
            .try_acquire()
            .map_err(|_| Error::Unavailable)?;
        let url = self
            .config
            .server
            .join(&format!("api/v1/query/{endpoint}"))
            .map_err(|_| Error::Configuration)?;
        let mut refreshed = false;
        for attempt in 0..3 {
            let token = self.token().await?;
            let response = self
                .http
                .get(url.clone())
                .query(query)
                .bearer_auth(token)
                .header("accept", "application/json")
                .send()
                .await;
            let result = match response {
                Ok(response)
                    if response.status() == reqwest::StatusCode::UNAUTHORIZED && !refreshed =>
                {
                    self.auth.lock().await.token = None;
                    refreshed = true;
                    continue;
                }
                Ok(response) => decode(response, 2 * 1024 * 1024).await,
                Err(_) => Err(Error::Network),
            };
            match result {
                Err(error) if error.retryable() && attempt < 2 => {
                    tokio::time::sleep(Duration::from_millis(250 * (attempt + 1))).await
                }
                result => return result,
            }
        }
        Err(Error::Unavailable)
    }
    pub async fn logout(&self) -> Result<(), Error> {
        let credentials = self.store.load().await?;
        // Revocation invalidates this connector's refresh token, not other CLI credential stores.
        let response = self
            .http
            .post("https://oauth2.googleapis.com/revoke")
            .form(&[("token", credentials.refresh_token.as_str())])
            .send()
            .await;
        self.auth.lock().await.token = None;
        self.store.delete().await?;
        if !response.is_ok_and(|r| r.status().is_success()) {
            return Err(Error::Network);
        }
        Ok(())
    }
}
pub async fn decode<T: DeserializeOwned>(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<T, Error> {
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status.is_redirection() {
        return Err(Error::LoginRequired);
    }
    if status == reqwest::StatusCode::FORBIDDEN {
        return Err(Error::Forbidden);
    }
    if status == reqwest::StatusCode::BAD_REQUEST {
        return Err(Error::Query);
    }
    if !status.is_success() {
        return Err(Error::Unavailable);
    }
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err(Error::Response);
    }
    let mut bytes = Vec::with_capacity(limit.min(16384));
    while let Some(chunk) = response.chunk().await.map_err(|_| Error::Network)? {
        if bytes.len() + chunk.len() > limit {
            return Err(Error::Response);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::Response)
}

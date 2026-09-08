//! Token exchanges reuse the shared rustls client and cannot accumulate oversized response bodies.
use azure_core::http::{AsyncRawResponse, Body, HttpClient, Request, headers::Headers};
use std::{future::Future, pin::Pin};
pub(crate) struct Transport {
    client: reqwest::Client,
}
impl Transport {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}
impl std::fmt::Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BoundedAzureTokenTransport")
    }
}
fn failure() -> azure_core::Error {
    azure_core::Error::new(
        azure_core::error::ErrorKind::Credential,
        "Azure token transport failed",
    )
}
impl HttpClient for Transport {
    fn execute_request<'life0, 'life1, 'async_trait>(
        &'life0 self,
        request: &'life1 Request,
    ) -> Pin<Box<dyn Future<Output = azure_core::Result<AsyncRawResponse>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let _permit = monitor_integrations::admission::acquire()
                .await
                .map_err(|_| failure())?;
            let url = request.url();
            if url.scheme() != "https"
                || url.host_str() != Some("login.microsoftonline.com")
                || !url.path().ends_with("/oauth2/v2.0/token")
                || request.method() != azure_core::http::Method::Post
            {
                return Err(failure());
            }
            let Body::Bytes(body) = request.body() else {
                return Err(failure());
            };
            if body.len() > 32768 {
                return Err(failure());
            }
            let response = self
                .client
                .post(url.as_str())
                .header("content-type", "application/x-www-form-urlencoded")
                .body(body.clone())
                .send()
                .await
                .map_err(|_| failure())?;
            let status = response.status().as_u16().into();
            let mut headers = Headers::new();
            headers.insert("content-type", "application/json");
            if let Some(delay) = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .filter(|v| v.len() <= 128)
            {
                headers.insert("retry-after", delay.to_owned());
            }
            let bytes = monitor_integrations::transport::bounded(response, 65536)
                .await
                .map_err(|_| failure())?;
            Ok(AsyncRawResponse::from_bytes(status, headers, bytes))
        })
    }
}

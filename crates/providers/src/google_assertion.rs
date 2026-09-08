//! Google metadata assertions are bounded and pinned to an explicit subject and audience.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use google_cloud_auth::credentials::idtoken::{IDTokenCredentials, mds};
use monitor_core::config::types::GoogleFederation;
use monitor_integrations::transport::Error;
use serde::Deserialize;
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct Assertion {
    credentials: IDTokenCredentials,
    identity: GoogleFederation,
    timeout: Duration,
    aws: bool,
}
impl std::fmt::Debug for Assertion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GoogleMetadataAssertion")
    }
}
impl Assertion {
    pub fn new(identity: GoogleFederation, timeout: Duration, aws: bool) -> Result<Self, Error> {
        let credentials = mds::Builder::new(identity.audience.clone())
            .with_endpoint("http://169.254.169.254")
            .build()
            .map_err(|_| Error::Authentication)?;
        Ok(Self {
            credentials,
            identity,
            timeout,
            aws,
        })
    }
    pub async fn token(&self) -> Result<String, Error> {
        let _permit = monitor_integrations::admission::acquire().await?;
        let token = tokio::time::timeout(self.timeout, self.credentials.id_token())
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Authentication)?;
        claims(
            &token,
            &self.identity,
            self.aws,
            chrono::Utc::now().timestamp(),
        )?;
        Ok(token)
    }
}
#[derive(Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: String,
    azp: Option<String>,
    exp: i64,
}
/// These checks bind the trusted metadata response; the receiving cloud verifies Google's signature.
pub(crate) fn claims(
    token: &str,
    expected: &GoogleFederation,
    aws: bool,
    now: i64,
) -> Result<(), Error> {
    if token.len() > 16384 {
        return Err(Error::Limit);
    }
    let mut parts = token.split('.');
    let header = parts.next().ok_or(Error::Authentication)?;
    let payload = parts.next().ok_or(Error::Authentication)?;
    let signature = parts.next().ok_or(Error::Authentication)?;
    if header.is_empty() || signature.is_empty() || parts.next().is_some() {
        return Err(Error::Authentication);
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| Error::Authentication)?;
    let value: Claims = serde_json::from_slice(&bytes).map_err(|_| Error::Authentication)?;
    if value.iss != "https://accounts.google.com"
        || value.sub != expected.subject
        || value.aud != expected.audience
        || value.exp <= now + 10
        || value.exp > now + 7200
        || aws && value.azp.as_deref() != Some(expected.subject.as_str())
    {
        return Err(Error::Authentication);
    }
    Ok(())
}

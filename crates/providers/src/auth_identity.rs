//! Identity assertions inspect native credential metadata; resource APIs validate the access token.
use base64::Engine;
use monitor_core::config::settings::Settings;
use monitor_integrations::{
    projection::text,
    transport::{Error, Http},
};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
fn claims(token: &str) -> Result<Value, Error> {
    if token.len() > 65536 {
        return Err(Error::Limit);
    }
    let payload = token.split('.').nth(1).ok_or(Error::Missing)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| Error::Missing)?;
    serde_json::from_slice(&bytes).map_err(|_| Error::Missing)
}
pub fn azure(token: &str, expected: &str) -> Result<(), Error> {
    let claims = claims(token)?;
    let candidates = ["/oid", "/upn", "/preferred_username"];
    if candidates.iter().any(|path| {
        text(&claims, &[path]).is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
    }) {
        Ok(())
    } else {
        Err(Error::Forbidden)
    }
}
pub async fn google(
    http: &Http,
    token: &str,
    expected: &str,
    settings: &Settings,
) -> Result<(), Error> {
    if let Ok(claims) = claims(token)
        && let Some(email) = text(&claims, &["/sub", "/iss"]).filter(|email| email.contains('@'))
    {
        return if email.eq_ignore_ascii_case(expected) {
            Ok(())
        } else {
            Err(Error::Forbidden)
        };
    }
    let request = http
        .client()
        .get("https://openidconnect.googleapis.com/v1/userinfo")
        .bearer_auth(token)
        .build()
        .map_err(|_| Error::Malformed)?;
    let value = http
        .json(request, settings, &CancellationToken::new())
        .await?;
    if text(&value, &["/email"]).is_some_and(|actual| actual.eq_ignore_ascii_case(expected)) {
        Ok(())
    } else {
        Err(Error::Forbidden)
    }
}

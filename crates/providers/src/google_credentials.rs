//! Named credential files are passed to native providers without changing process environment.
use google_cloud_auth::credentials::{self, AccessTokenCredentials};
use monitor_core::config::types::Credential;
use monitor_integrations::transport::Error;
use tokio::io::AsyncReadExt;
pub async fn load(profile: Option<&Credential>) -> Result<AccessTokenCredentials, Error> {
    let scopes = if profile.is_some_and(|profile| profile.expected_identity.is_some()) {
        vec![
            "https://www.googleapis.com/auth/cloud-platform",
            "https://www.googleapis.com/auth/userinfo.email",
        ]
    } else {
        vec!["https://www.googleapis.com/auth/cloud-platform"]
    };
    let Some(path) = profile.and_then(|profile| profile.credential_file.as_ref()) else {
        return credentials::Builder::default()
            .with_scopes(scopes)
            .build_access_token_credentials()
            .map_err(|_| Error::Authentication);
    };
    let mut bytes = Vec::new();
    tokio::fs::File::open(path)
        .await
        .map_err(|_| Error::Authentication)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| Error::Authentication)?;
    if bytes.len() > 1024 * 1024 {
        return Err(Error::Limit);
    }
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| Error::Authentication)?;
    match value.get("type").and_then(|kind| kind.as_str()) {
        Some("service_account") => credentials::service_account::Builder::new(value)
            .with_access_specifier(credentials::service_account::AccessSpecifier::from_scopes(
                scopes,
            ))
            .build_access_token_credentials(),
        Some("authorized_user") => credentials::user_account::Builder::new(value)
            .with_scopes(scopes)
            .build_access_token_credentials(),
        Some("external_account") => credentials::external_account::Builder::new(value)
            .with_scopes(scopes)
            .build_access_token_credentials(),
        Some("impersonated_service_account") => credentials::impersonated::Builder::new(value)
            .with_scopes(scopes)
            .build_access_token_credentials(),
        _ => return Err(Error::Authentication),
    }
    .map_err(|_| Error::Authentication)
}

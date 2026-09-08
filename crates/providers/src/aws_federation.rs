//! Native STS exchanges Google assertions; no AWS login files are needed on the monitoring VM.
use crate::google_assertion::Assertion;
use aws_credential_types::{
    Credentials,
    provider::{ProvideCredentials, SharedCredentialsProvider, error::CredentialsError, future},
};
use monitor_core::config::{settings::Settings, types::Credential};
use monitor_integrations::transport::{Error, Http};
use std::time::SystemTime;

struct Federated {
    assertion: Assertion,
    sts: aws_sdk_sts::Client,
    role: String,
}
impl std::fmt::Debug for Federated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GoogleFederatedAwsCredentials")
    }
}
fn failure() -> CredentialsError {
    CredentialsError::provider_error("Google to AWS credential exchange failed")
}
impl ProvideCredentials for Federated {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        future::ProvideCredentials::new(async move {
            let assertion = self.assertion.token().await.map_err(|_| failure())?;
            let response = self
                .sts
                .assume_role_with_web_identity()
                .role_arn(&self.role)
                .role_session_name("healthcheck-monitor")
                .web_identity_token(assertion)
                .duration_seconds(3600)
                .send()
                .await
                .map_err(|_| failure())?;
            let value = response.credentials().ok_or_else(failure)?;
            let expires: SystemTime = value
                .expiration()
                .to_owned()
                .try_into()
                .map_err(|_| failure())?;
            if value.access_key_id().is_empty()
                || value.secret_access_key().is_empty()
                || value.session_token().is_empty()
                || expires <= SystemTime::now()
            {
                return Err(failure());
            }
            Ok(Credentials::new(
                value.access_key_id(),
                value.secret_access_key(),
                Some(value.session_token().into()),
                Some(expires),
                "GoogleFederation",
            ))
        })
    }
}
pub(crate) async fn config(
    profile: &Credential,
    region: &str,
    http: &Http,
    settings: &Settings,
) -> Result<aws_config::SdkConfig, Error> {
    profile.validate().map_err(|_| Error::Forbidden)?;
    let identity = profile
        .google_federation
        .clone()
        .ok_or(Error::Authentication)?;
    let role = profile.role_arn.clone().ok_or(Error::Authentication)?;
    let base = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .no_credentials()
        .region(aws_config::Region::new(region.to_owned()))
        .disable_request_compression(true)
        .http_client(crate::aws_transport::client(http, settings)?)
        .timeout_config(
            aws_config::timeout::TimeoutConfig::builder()
                .connect_timeout(settings.connect_timeout.duration())
                .operation_attempt_timeout(settings.attempt_timeout.duration())
                .operation_timeout(settings.operation_timeout.duration())
                .build(),
        )
        .retry_config(
            aws_config::retry::RetryConfig::standard().with_max_attempts(settings.attempts as u32),
        )
        .load()
        .await;
    // Pin the assertion destination independently of optional SDK endpoint environment settings.
    let sts = aws_sdk_sts::Client::from_conf(
        aws_sdk_sts::config::Builder::from(&base)
            .region(aws_config::Region::new("us-east-1"))
            .endpoint_url("https://sts.us-east-1.amazonaws.com")
            .build(),
    );
    let provider = Federated {
        assertion: Assertion::new(identity, settings.attempt_timeout.duration(), true)?,
        sts,
        role,
    };
    Ok(base
        .to_builder()
        .credentials_provider(SharedCredentialsProvider::new(provider))
        .build())
}

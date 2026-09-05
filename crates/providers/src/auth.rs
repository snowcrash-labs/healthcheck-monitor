//! Persistent native credential providers; collection never initiates login.
use azure_core::credentials::TokenCredential;
use monitor_core::{config::types::Credential, model::Provider};
use monitor_integrations::transport::Error;
use std::sync::Arc;

pub enum Auth {
    Gcp(google_cloud_auth::credentials::AccessTokenCredentials),
    Azure(Arc<dyn TokenCredential>),
    Aws(Box<crate::aws_clients::AwsClients>),
    None,
}
impl Auth {
    pub async fn new(
        provider: Provider,
        profile: Option<&Credential>,
        region: Option<&str>,
        scope: Option<&str>,
        http: &monitor_integrations::transport::Http,
        settings: &monitor_core::config::settings::Settings,
    ) -> Result<Self, Error> {
        match provider {
            Provider::Gcp => {
                let credentials = crate::google_credentials::load(profile).await?;
                if let Some(expected) =
                    profile.and_then(|profile| profile.expected_identity.as_deref())
                {
                    let token = credentials
                        .access_token()
                        .await
                        .map_err(|_| Error::Authentication)?;
                    crate::auth_identity::google(http, &token.token, expected, settings).await?;
                }
                Ok(Self::Gcp(credentials))
            }
            Provider::Azure => {
                let credential: Arc<dyn TokenCredential> = if profile
                    .and_then(|c| c.profile.as_deref())
                    == Some("managed_identity")
                {
                    azure_identity::ManagedIdentityCredential::new(None)
                        .map_err(|_| Error::Authentication)?
                } else if profile.and_then(|c| c.profile.as_deref()) == Some("workload_identity") {
                    azure_identity::WorkloadIdentityCredential::new(Some(
                        azure_identity::WorkloadIdentityCredentialOptions {
                            tenant_id: profile.and_then(|p| p.tenant.clone()),
                            ..Default::default()
                        },
                    ))
                    .map_err(|_| Error::Authentication)?
                } else {
                    azure_identity::AzureCliCredential::new(Some(
                        azure_identity::AzureCliCredentialOptions {
                            tenant_id: profile.and_then(|p| p.tenant.clone()),
                            ..Default::default()
                        },
                    ))
                    .map_err(|_| Error::Authentication)?
                };
                if let Some(expected) =
                    profile.and_then(|profile| profile.expected_identity.as_deref())
                {
                    let token = credential
                        .get_token(&["https://management.azure.com/.default"], None)
                        .await
                        .map_err(|_| Error::Authentication)?;
                    crate::auth_identity::azure(token.token.secret(), expected)?;
                }
                Ok(Self::Azure(credential))
            }
            Provider::Aws => {
                let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
                    // Buffered requests allow body caps to apply before transmission.
                    .disable_request_compression(true)
                    .http_client(crate::aws_transport::client(http, settings)?)
                    .region(aws_config::Region::new(
                        region.unwrap_or("us-east-1").to_string(),
                    ))
                    .timeout_config(
                        aws_config::timeout::TimeoutConfig::builder()
                            .connect_timeout(settings.connect_timeout.duration())
                            .operation_timeout(settings.operation_timeout.duration())
                            .operation_attempt_timeout(settings.attempt_timeout.duration())
                            .build(),
                    )
                    .retry_config(
                        aws_config::retry::RetryConfig::standard()
                            .with_max_attempts(settings.attempts as u32),
                    );
                if let Some(name) = profile.and_then(|p| p.profile.as_ref()) {
                    loader = loader.profile_name(name);
                }
                let mut config = loader.load().await;
                if let Some(role) = profile.and_then(|p| p.role_arn.as_ref()) {
                    let provider = aws_config::sts::AssumeRoleProvider::builder(role)
                        .session_name("healthcheck-monitor")
                        .configure(&config)
                        .build()
                        .await;
                    config = config
                        .to_builder()
                        .credentials_provider(
                            aws_credential_types::provider::SharedCredentialsProvider::new(
                                provider,
                            ),
                        )
                        .build();
                }
                let clients = crate::aws_clients::AwsClients::new(config);
                let identity = clients
                    .sts
                    .get_caller_identity()
                    .send()
                    .await
                    .map_err(|_| Error::Authentication)?;
                if scope.is_some_and(|scope| identity.account() != Some(scope)) {
                    return Err(Error::Forbidden);
                }
                if profile
                    .and_then(|profile| profile.expected_identity.as_deref())
                    .is_some_and(|expected| identity.arn() != Some(expected))
                {
                    return Err(Error::Forbidden);
                }
                Ok(Self::Aws(Box::new(clients)))
            }
            _ => Ok(Self::None),
        }
    }
    pub async fn bearer(&self) -> Result<String, Error> {
        self.bearer_for(false).await
    }
    pub async fn registry_bearer(&self) -> Result<String, Error> {
        let Self::Azure(credentials) = self else {
            return Err(Error::Authentication);
        };
        credentials
            .get_token(&["https://containerregistry.azure.net/.default"], None)
            .await
            .map(|token| token.token.secret().to_string())
            .map_err(|_| Error::Authentication)
    }
    pub async fn bearer_for(&self, logs: bool) -> Result<String, Error> {
        match self {
            Self::Gcp(credentials) => credentials
                .access_token()
                .await
                .map(|t| t.token)
                .map_err(|_| Error::Authentication),
            Self::Azure(credentials) => credentials
                .get_token(
                    &[if logs {
                        "https://api.loganalytics.io/.default"
                    } else {
                        "https://management.azure.com/.default"
                    }],
                    None,
                )
                .await
                .map(|t| t.token.secret().to_string())
                .map_err(|_| Error::Authentication),
            _ => Err(Error::Authentication),
        }
    }
    /// AWS signing uses refreshable native credentials for each operation.
    pub async fn sign(
        &self,
        request: &mut reqwest::Request,
        service: &str,
        region: &str,
    ) -> Result<(), Error> {
        use aws_credential_types::provider::ProvideCredentials;
        use aws_sigv4::{
            http_request::{SignableBody, SignableRequest, SigningSettings, sign},
            sign::v4,
        };
        let Self::Aws(clients) = self else {
            return Err(Error::Authentication);
        };
        let identity = clients
            .config
            .credentials_provider()
            .ok_or(Error::Authentication)?
            .provide_credentials()
            .await
            .map_err(|_| Error::Authentication)?
            .into();
        let settings = v4::SigningParams::builder()
            .identity(&identity)
            .region(region)
            .name(service)
            .time(std::time::SystemTime::now())
            .settings(SigningSettings::default())
            .build()
            .map_err(|_| Error::Authentication)?
            .into();
        let body = request.body().and_then(|b| b.as_bytes()).unwrap_or(&[]);
        let headers = request
            .headers()
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str(), v)));
        let signable = SignableRequest::new(
            request.method().as_str(),
            request.url().as_str(),
            headers,
            SignableBody::Bytes(body),
        )
        .map_err(|_| Error::Malformed)?;
        let (instructions, _) = sign(signable, &settings)
            .map_err(|_| Error::Authentication)?
            .into_parts();
        for (name, value) in instructions.headers() {
            let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| Error::Malformed)?;
            let value =
                reqwest::header::HeaderValue::from_str(value).map_err(|_| Error::Malformed)?;
            request.headers_mut().insert(name, value);
        }
        Ok(())
    }
}

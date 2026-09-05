//! Persistent native credential providers; collection never initiates login.
use azure_core::credentials::TokenCredential;
use monitor_core::{config::types::Credential, model::Provider};
use monitor_integrations::transport::Error;
use std::sync::Arc;

pub enum Auth {
    Gcp(google_cloud_auth::credentials::AccessTokenCredentials),
    Azure(Box<crate::azure_auth::Credential>),
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
        processes: &monitor_integrations::process::Processes,
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
                            subscription: scope.map(String::from),
                            executor: Some(Arc::new(crate::azure_auth::Executor {
                                processes: processes.clone(),
                                timeout: settings.attempt_timeout.duration(),
                                limit: settings.response_bytes.min(65536),
                            })),
                        },
                    ))
                    .map_err(|_| Error::Authentication)?
                };
                let credential = crate::azure_auth::Credential::new(credential);
                if let Some(expected) =
                    profile.and_then(|profile| profile.expected_identity.as_deref())
                {
                    let token = credential
                        .bearer(crate::azure_auth::Audience::Management)
                        .await?;
                    crate::auth_identity::azure(&token, expected)?;
                }
                Ok(Self::Azure(Box::new(credential)))
            }
            Provider::Aws => {
                let prepared = crate::aws_process_profiles::load(
                    profile.and_then(|profile| profile.profile.as_deref()),
                )
                .await?;
                let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
                    .profile_files(prepared.files.clone())
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
                if let Some(provider) = crate::aws_process_chain::build(
                    &prepared,
                    region.unwrap_or("us-east-1"),
                    http,
                    settings,
                    processes,
                )? {
                    loader = loader.credentials_provider(provider);
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
            .bearer(crate::azure_auth::Audience::Registry)
            .await
    }
    pub async fn vault_bearer(&self) -> Result<String, Error> {
        let Self::Azure(credentials) = self else {
            return Err(Error::Authentication);
        };
        credentials.bearer(crate::azure_auth::Audience::Vault).await
    }
    pub async fn bearer_for(&self, logs: bool) -> Result<String, Error> {
        match self {
            Self::Gcp(credentials) => credentials
                .access_token()
                .await
                .map(|t| t.token)
                .map_err(|_| Error::Authentication),
            Self::Azure(credentials) => {
                credentials
                    .bearer(if logs {
                        crate::azure_auth::Audience::Logs
                    } else {
                        crate::azure_auth::Audience::Management
                    })
                    .await
            }
            _ => Err(Error::Authentication),
        }
    }
}

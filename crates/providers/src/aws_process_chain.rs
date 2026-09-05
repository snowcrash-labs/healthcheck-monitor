//! Keep native environment, profile, web identity and workload providers around bounded process sources.
use crate::aws_process_profiles::Prepared;
use aws_config::{meta::credentials::CredentialsProviderChain, provider_config::ProviderConfig};
use monitor_core::config::settings::Settings;
use monitor_integrations::{
    process::Processes,
    transport::{Error, Http},
};
pub fn build(
    prepared: &Prepared,
    region: &str,
    http: &Http,
    settings: &Settings,
    processes: &Processes,
) -> Result<Option<CredentialsProviderChain>, Error> {
    let Some(command) = &prepared.command else {
        return Ok(None);
    };
    let config = ProviderConfig::default()
        .with_behavior_version(Some(aws_config::BehaviorVersion::latest()))
        .with_region(Some(aws_config::Region::new(region.to_owned())))
        .with_http_client(crate::aws_transport::client(http, settings)?)
        .with_timeout_config(
            aws_config::timeout::TimeoutConfig::builder()
                .connect_timeout(settings.connect_timeout.duration())
                .operation_timeout(settings.operation_timeout.duration())
                .operation_attempt_timeout(settings.attempt_timeout.duration())
                .build(),
        )
        .with_retry_config(
            aws_config::retry::RetryConfig::standard().with_max_attempts(settings.attempts as u32),
        );
    let profile = aws_config::profile::ProfileFileCredentialsProvider::builder()
        .configure(&config)
        .profile_files(prepared.files.clone())
        .profile_name(&prepared.selected)
        .with_custom_provider(
            "HealthcheckCredentialProcess",
            crate::aws_credential_process::Process::new(
                command.clone(),
                processes.clone(),
                settings.attempt_timeout.duration(),
                settings.response_bytes.min(65536),
            ),
        )
        .build();
    Ok(Some(
        CredentialsProviderChain::first_try(
            "Environment",
            aws_config::environment::credentials::EnvironmentVariableCredentialsProvider::new(),
        )
        .or_else("Profile", profile)
        .or_else(
            "WebIdentityToken",
            aws_config::web_identity_token::WebIdentityTokenCredentialsProvider::builder()
                .configure(&config)
                .build(),
        )
        .or_else(
            "EcsContainer",
            aws_config::ecs::EcsCredentialsProvider::builder()
                .configure(&config)
                .build(),
        )
        .or_else(
            "Ec2InstanceMetadata",
            aws_config::imds::credentials::ImdsCredentialsProvider::builder()
                .configure(&config)
                .build(),
        ),
    ))
}

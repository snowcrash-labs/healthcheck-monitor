//! Profile roles retain native SDK behavior while process credentials remain bounded and cancellable.
use crate::{
    aws_credential_process::Process,
    aws_process_profiles::{Prepared, prepare},
};
use aws_credential_types::provider::ProvideCredentials;
use monitor_integrations::process::Processes;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
const COMMAND: &str = "printf '%s' '{\"Version\":1,\"AccessKeyId\":\"synthetic-key\",\"SecretAccessKey\":\"synthetic-secret\",\"SessionToken\":\"synthetic-session\"}'";
fn provider(
    prepared: &Prepared,
    config: &aws_config::provider_config::ProviderConfig,
) -> Result<aws_config::profile::ProfileFileCredentialsProvider, Box<dyn std::error::Error>> {
    Ok(
        aws_config::profile::ProfileFileCredentialsProvider::builder()
            .configure(config)
            .profile_files(prepared.files.clone())
            .profile_name(&prepared.selected)
            .with_custom_provider(
                "HealthcheckCredentialProcess",
                Process::new(
                    prepared.command.clone().ok_or("missing command")?,
                    Processes::new(1),
                    Duration::from_secs(2),
                    65536,
                ),
            )
            .build(),
    )
}
#[tokio::test]
async fn sdk_profile_parser_selects_the_bounded_process_source()
-> Result<(), Box<dyn std::error::Error>> {
    for (config, credentials) in [
        (
            format!("[profile app]\ncredential_process = {COMMAND}\n"),
            String::new(),
        ),
        (
            String::new(),
            format!("[app]\ncredential_process = {COMMAND}\n"),
        ),
    ] {
        let prepared = prepare(config, credentials, Some("app")).await?;
        let provider = provider(
            &prepared,
            &aws_config::provider_config::ProviderConfig::default(),
        )?;
        let credentials = provider.provide_credentials().await?;
        assert_eq!(credentials.access_key_id(), "synthetic-key");
        assert_eq!(credentials.session_token(), Some("synthetic-session"));
    }
    Ok(())
}
#[tokio::test]
async fn unused_process_profiles_do_not_override_sso_or_named_sources()
-> Result<(), Box<dyn std::error::Error>> {
    for source in [
        "sso_session = session",
        "credential_source = Environment",
        "web_identity_token_file = /configured/token",
    ] {
        let prepared=prepare(format!("[profile selected]\n{source}\ncredential_process = unused-command\n[profile other]\ncredential_process = unused-command\n"),String::new(),Some("selected")).await?;
        assert!(prepared.command.is_none());
    }
    assert!(
        prepare("x".repeat(1024 * 1024 + 1), String::new(), None)
            .await
            .is_err()
    );
    Ok(())
}
#[tokio::test]
async fn process_failures_and_cancellation_release_capacity_without_exposing_arguments()
-> Result<(), Box<dyn std::error::Error>> {
    let processes = Processes::new(1);
    let oversized = Process::new(
        "while :; do printf '0123456789'; done".into(),
        processes.clone(),
        Duration::from_secs(2),
        64,
    );
    assert!(oversized.provide_credentials().await.is_err());
    let slow = Process::new(
        "sleep 60 & wait # sensitive-argument".into(),
        processes.clone(),
        Duration::from_secs(60),
        1024,
    );
    assert!(!format!("{slow:?}").contains("sensitive-argument"));
    assert!(
        tokio::time::timeout(Duration::from_millis(20), slow.provide_credentials())
            .await
            .is_err()
    );
    assert_eq!(processes.budget().available_permits(), 1);
    let valid = Process::new(COMMAND.into(), processes, Duration::from_secs(2), 65536);
    assert_eq!(
        valid.provide_credentials().await?.access_key_id(),
        "synthetic-key"
    );
    Ok(())
}
#[derive(Debug, Clone)]
struct Sts(Arc<AtomicUsize>);
impl aws_smithy_runtime_api::client::http::HttpConnector for Sts {
    fn call(
        &self,
        request: aws_smithy_runtime_api::client::orchestrator::HttpRequest,
    ) -> aws_smithy_runtime_api::client::http::HttpConnectorFuture {
        self.0.fetch_add(1, Ordering::SeqCst);
        assert!(
            request
                .body()
                .bytes()
                .is_some_and(|body| String::from_utf8_lossy(body).contains("Action=AssumeRole"))
        );
        aws_smithy_runtime_api::client::http::HttpConnectorFuture::new(async {
            let status = aws_smithy_runtime_api::http::StatusCode::try_from(200).map_err(|_| {
                aws_smithy_runtime_api::client::result::ConnectorError::other(
                    Box::new(std::io::Error::other("fixture status")),
                    None,
                )
            })?;
            Ok(
                aws_smithy_runtime_api::client::orchestrator::HttpResponse::new(
                    status,
                    aws_smithy_types::body::SdkBody::from(
                        "<AssumeRoleResponse xmlns=\"https://sts.amazonaws.com/doc/2011-06-15/\"><AssumeRoleResult><Credentials><AccessKeyId>assumed-key</AccessKeyId><SecretAccessKey>synthetic-secret</SecretAccessKey><SessionToken>synthetic-session</SessionToken><Expiration>2100-01-01T00:00:00Z</Expiration></Credentials><AssumedRoleUser><Arn>arn:aws:sts::123456789012:assumed-role/app/session</Arn><AssumedRoleId>synthetic-id</AssumedRoleId></AssumedRoleUser></AssumeRoleResult></AssumeRoleResponse>",
                    ),
                ),
            )
        })
    }
}
#[tokio::test]
async fn source_profile_and_self_reference_roles_still_use_native_sts()
-> Result<(), Box<dyn std::error::Error>> {
    use aws_smithy_runtime_api::client::http::{SharedHttpConnector, http_client_fn};
    for config in [
        format!(
            "[profile app]\nrole_arn = arn:aws:iam::123456789012:role/app\nsource_profile = base\n[profile base]\ncredential_process = {COMMAND}\n"
        ),
        format!(
            "[profile app]\nrole_arn = arn:aws:iam::123456789012:role/app\nsource_profile = app\ncredential_process = {COMMAND}\n"
        ),
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let connector = SharedHttpConnector::new(Sts(calls.clone()));
        let configuration = aws_config::provider_config::ProviderConfig::default()
            .with_region(Some(aws_config::Region::new("us-east-1")))
            .with_http_client(http_client_fn(move |_, _| connector.clone()))
            .with_behavior_version(Some(aws_config::BehaviorVersion::latest()));
        let prepared = prepare(config, String::new(), Some("app")).await?;
        let credentials = provider(&prepared, &configuration)?
            .provide_credentials()
            .await?;
        assert_eq!(credentials.access_key_id(), "assumed-key");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

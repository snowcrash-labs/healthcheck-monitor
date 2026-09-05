//! Credential plugin execution is noninteractive, bounded, cancellable, and cached until refresh.
use super::*;
use std::time::Duration;
fn exec(script: &str) -> ExecConfig {
    ExecConfig {
        command: Some("/bin/sh".into()),
        args: Some(vec!["-c".into(), script.into()]),
        api_version: Some("client.authentication.k8s.io/v1beta1".into()),
        ..Default::default()
    }
}
fn job() -> Result<Job, Box<dyn std::error::Error>> {
    monitor_core::config::types::Config::parse(
        "version=1\n[[targets]]\nname='kube'\nprovider='kubernetes'\nscope='kube'",
    )?
    .resolve(&Default::default())?
    .jobs
    .into_iter()
    .next()
    .ok_or_else(|| "missing job".into())
}
#[tokio::test]
async fn plugin_input_never_requests_interactive_authentication()
-> Result<(), Box<dyn std::error::Error>> {
    let plugin = exec("printf '%s' \"$KUBERNETES_EXEC_INFO\"");
    let output = Processes::new(1)
        .run_command(
            command(&plugin)?,
            4096,
            Duration::from_secs(2),
            &CancellationToken::new(),
        )
        .await?;
    let info: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(
        info.pointer("/spec/interactive"),
        Some(&serde_json::json!(false))
    );
    let mut interactive = plugin;
    interactive.interactive_mode = Some(ExecInteractiveMode::Always);
    assert!(matches!(command(&interactive), Err(Error::Authentication)));
    Ok(())
}
#[tokio::test]
async fn credential_output_cap_and_timeout_release_process_capacity()
-> Result<(), Box<dyn std::error::Error>> {
    let processes = Processes::new(1);
    let output = processes
        .run_command(
            command(&exec("while :; do printf '0123456789'; done"))?,
            64,
            Duration::from_secs(2),
            &CancellationToken::new(),
        )
        .await;
    assert!(matches!(output, Err(Error::Limit)));
    assert_eq!(processes.budget().available_permits(), 1);
    let held = processes.budget().acquire().await?;
    let output = processes
        .run_command(
            command(&exec("printf never"))?,
            64,
            Duration::from_millis(10),
            &CancellationToken::new(),
        )
        .await;
    assert!(matches!(output, Err(Error::Timeout)));
    drop(held);
    assert_eq!(processes.budget().available_permits(), 1);
    Ok(())
}
#[tokio::test]
async fn cached_client_is_reused_and_expired_credentials_trigger_reacquisition()
-> Result<(), Box<dyn std::error::Error>> {
    let mut config = kube::Config::new("https://127.0.0.1:6443".parse()?);
    config.auth_info.exec = Some(exec(
        "printf '%s' '{\"apiVersion\":\"client.authentication.k8s.io/v1beta1\",\"kind\":\"ExecCredential\",\"status\":{\"token\":\"synthetic-token\"}}'",
    ));
    let mut session = Session::new(config, Processes::new(1))?;
    let job = job()?;
    let cancel = CancellationToken::new();
    let _ = session.client(&job, &cancel).await?;
    session.exec.as_mut().ok_or("missing plugin")?.command =
        Some("healthcheck-missing-auth-fixture".into());
    assert!(session.client(&job, &cancel).await.is_ok());
    session.expires = Some(Utc::now() - chrono::Duration::seconds(1));
    assert!(matches!(
        session.client(&job, &cancel).await,
        Err(Error::Unavailable)
    ));
    Ok(())
}
#[test]
fn invalid_version_empty_credentials_and_expiry_fail_closed() {
    for value in [
        serde_json::json!({"apiVersion":"wrong","kind":"ExecCredential","status":{"token":"token"}}),
        serde_json::json!({"apiVersion":"client.authentication.k8s.io/v1beta1","kind":"ExecCredential","status":{}}),
        serde_json::json!({"apiVersion":"client.authentication.k8s.io/v1beta1","kind":"ExecCredential","status":{"token":"token","expirationTimestamp":"2000-01-01T00:00:00Z"}}),
    ] {
        let encoded = value.to_string();
        assert!(parse(encoded.as_bytes(), None).is_err());
    }
}
#[tokio::test]
async fn legacy_gcp_command_credentials_remain_bounded_and_supported()
-> Result<(), Box<dyn std::error::Error>> {
    let provider=kube::config::AuthProviderConfig{name:"gcp".into(),config:std::collections::HashMap::from([("cmd-path".into(),"/bin/echo".into()),("cmd-args".into(),"{\"credential\":{\"access_token\":\"synthetic-token\",\"token_expiry\":\"2100-01-01T00:00:00Z\"}}".into()),("token-key".into(),"{.credential.access_token}".into()),("expiry-key".into(),"{.credential.token_expiry}".into())]),..Default::default()};
    let status = legacy::acquire(
        &provider,
        &Processes::new(1),
        &job()?,
        &CancellationToken::new(),
    )
    .await?;
    assert_eq!(status.token.as_deref(), Some("synthetic-token"));
    assert!(status.expiration_timestamp.is_some());
    Ok(())
}

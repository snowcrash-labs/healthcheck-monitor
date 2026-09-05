//! Native Kubernetes clients use bounded, noninteractive exec credential acquisition.
use crate::{process::Processes, transport::Error};
use base64::Engine;
use chrono::{DateTime, Utc};
use kube::config::{ExecConfig, ExecInteractiveMode};
use monitor_core::config::resolve::Job;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;
pub(crate) struct Session {
    config: kube::Config,
    exec: Option<ExecConfig>,
    legacy: Option<kube::config::AuthProviderConfig>,
    client: Option<kube::Client>,
    transport: Option<(u64, u64)>,
    expires: Option<DateTime<Utc>>,
    processes: Processes,
}
impl Session {
    pub fn new(mut config: kube::Config, processes: Processes) -> Result<Self, Error> {
        let legacy = if config
            .auth_info
            .auth_provider
            .as_ref()
            .is_some_and(|provider| provider.name == "gcp")
        {
            config.auth_info.auth_provider.take()
        } else {
            None
        };
        let exec = config.auth_info.exec.take();
        Ok(Self {
            config,
            exec,
            legacy,
            client: None,
            transport: None,
            expires: None,
            processes,
        })
    }
    pub async fn client(
        &mut self,
        job: &Job,
        cancel: &CancellationToken,
    ) -> Result<kube::Client, Error> {
        let fresh = self.client.is_some()
            && self
                .expires
                .is_none_or(|until| until > Utc::now() + chrono::Duration::seconds(30));
        let transport = (
            job.settings.connect_timeout.0,
            job.settings.attempt_timeout.0,
        );
        if fresh
            && self.transport == Some(transport)
            && let Some(client) = &self.client
        {
            return Ok(client.clone());
        }
        if !fresh {
            if let Some(exec) = &self.exec {
                let command = command(exec)?;
                let output = self
                    .processes
                    .run_command(
                        command,
                        job.settings.response_bytes.min(65536),
                        job.settings.attempt_timeout.duration(),
                        cancel,
                    )
                    .await?;
                let credential = parse(&output.stdout, exec.api_version.as_deref())?;
                self.expires = credential.expiration_timestamp;
                self.config.auth_info.token = credential.token.map(Into::into);
                self.config.auth_info.token_file = None;
                self.config.auth_info.client_certificate = None;
                self.config.auth_info.client_key = None;
                self.config.auth_info.client_certificate_data = credential
                    .client_certificate_data
                    .map(|value| base64::prelude::BASE64_STANDARD.encode(value));
                self.config.auth_info.client_key_data = credential
                    .client_key_data
                    .map(|value| base64::prelude::BASE64_STANDARD.encode(value).into());
            } else if let Some(provider) = &self.legacy {
                let status = legacy::acquire(provider, &self.processes, job, cancel).await?;
                self.config.auth_info.token = status.token.map(Into::into);
                self.config.auth_info.token_file = None;
                self.expires = status.expiration_timestamp;
            }
        }
        self.config.connect_timeout = Some(job.settings.connect_timeout.duration());
        self.config.read_timeout = Some(job.settings.attempt_timeout.duration());
        self.config.write_timeout = Some(job.settings.attempt_timeout.duration());
        let client =
            kube::Client::try_from(self.config.clone()).map_err(|_| Error::Authentication)?;
        self.client = Some(client.clone());
        self.transport = Some(transport);
        Ok(client)
    }
}
#[path = "kube_legacy_auth.rs"]
mod legacy;
#[cfg(test)]
#[path = "kube_auth_tests.rs"]
mod tests;
/// The exec configuration comes from the selected kubeconfig, never a collection command hook.
pub(crate) fn command(exec: &ExecConfig) -> Result<tokio::process::Command, Error> {
    let version = exec
        .api_version
        .as_deref()
        .unwrap_or("client.authentication.k8s.io/v1beta1");
    if !matches!(
        version,
        "client.authentication.k8s.io/v1" | "client.authentication.k8s.io/v1beta1"
    ) || exec.interactive_mode == Some(ExecInteractiveMode::Always)
    {
        return Err(Error::Authentication);
    }
    let executable = exec
        .command
        .as_ref()
        .filter(|command| !command.is_empty() && command.len() <= 4096)
        .ok_or(Error::Authentication)?;
    if exec
        .args
        .as_ref()
        .is_some_and(|args| args.len() > 128 || args.iter().any(|arg| arg.len() > 4096))
        || exec.env.as_ref().is_some_and(|env| env.len() > 128)
    {
        return Err(Error::Limit);
    }
    let mut command = tokio::process::Command::new(executable);
    if let Some(args) = &exec.args {
        command.args(args);
    }
    for env in exec.env.iter().flatten() {
        let name = env.get("name").ok_or(Error::Malformed)?;
        let value = env.get("value").ok_or(Error::Malformed)?;
        if name.is_empty() || name.len() > 256 || name.contains(['=', '\0']) || value.len() > 65536
        {
            return Err(Error::Limit);
        }
        command.env(name, value);
    }
    let mut spec = serde_json::json!({"interactive":false});
    if exec.provide_cluster_info {
        spec["cluster"] = serde_json::to_value(exec.cluster.as_ref().ok_or(Error::Authentication)?)
            .map_err(|_| Error::Malformed)?;
    }
    let info = serde_json::to_string(
        &serde_json::json!({"apiVersion":version,"kind":"ExecCredential","spec":spec}),
    )
    .map_err(|_| Error::Malformed)?;
    if info.len() > 65536 {
        return Err(Error::Limit);
    }
    command.env("KUBERNETES_EXEC_INFO", info);
    for name in exec.drop_env.iter().flatten() {
        command.env_remove(name);
    }
    Ok(command)
}
#[derive(Deserialize)]
struct Credential {
    #[serde(rename = "apiVersion")]
    version: String,
    kind: String,
    status: Status,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    token: Option<String>,
    expiration_timestamp: Option<DateTime<Utc>>,
    client_certificate_data: Option<String>,
    client_key_data: Option<String>,
}
pub(crate) fn parse(bytes: &[u8], version: Option<&str>) -> Result<Status, Error> {
    let credential: Credential = serde_json::from_slice(bytes).map_err(|_| Error::Malformed)?;
    let status = credential.status;
    if credential.kind != "ExecCredential"
        || credential.version != version.unwrap_or("client.authentication.k8s.io/v1beta1")
        || status
            .expiration_timestamp
            .is_some_and(|until| until <= Utc::now())
    {
        return Err(Error::Authentication);
    }
    let token = status
        .token
        .as_ref()
        .is_some_and(|token| !token.is_empty() && !token.chars().any(char::is_control));
    let certificate = status
        .client_certificate_data
        .as_ref()
        .is_some_and(|value| value.contains("BEGIN CERTIFICATE"))
        && status
            .client_key_data
            .as_ref()
            .is_some_and(|value| value.contains("PRIVATE KEY"));
    if !token && !certificate {
        return Err(Error::Authentication);
    }
    Ok(status)
}

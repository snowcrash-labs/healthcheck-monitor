//! Refreshable process credentials stay bounded and never enter evidence or logs.
use aws_credential_types::{
    Credentials,
    provider::{self, ProvideCredentials, error::CredentialsError, future},
};
use monitor_integrations::process::Processes;
use serde::Deserialize;
use std::{
    fmt,
    time::{Duration, SystemTime},
};
use tokio::sync::Mutex;
pub struct Process {
    command: String,
    processes: Processes,
    timeout: Duration,
    limit: usize,
    cached: Mutex<Option<Credentials>>,
}
impl Process {
    pub fn new(command: String, processes: Processes, timeout: Duration, limit: usize) -> Self {
        Self {
            command,
            processes,
            timeout,
            limit,
            cached: Mutex::new(None),
        }
    }
    async fn credentials(&self) -> provider::Result {
        let mut cached = self.cached.lock().await;
        let now = SystemTime::now();
        if let Some(credentials) = cached.as_ref().filter(|credentials| {
            credentials
                .expiry()
                .is_none_or(|expiry| expiry > now + Duration::from_secs(60))
        }) {
            return Ok(credentials.clone());
        }
        let mut command = tokio::process::Command::new("/bin/sh");
        command.args(["-c", &self.command]);
        let output = self
            .processes
            .run_command(
                command,
                self.limit,
                self.timeout,
                &tokio_util::sync::CancellationToken::new(),
            )
            .await
            .map_err(|_| CredentialsError::provider_error("credential helper failed"))?;
        let value: Response = serde_json::from_slice(&output.stdout)
            .map_err(|_| CredentialsError::provider_error("invalid credential response"))?;
        if value.version != 1
            || value.access_key_id.is_empty()
            || value.secret_access_key.is_empty()
            || value
                .expiration
                .is_some_and(|expiry| expiry <= chrono::Utc::now())
        {
            return Err(CredentialsError::provider_error(
                "invalid or expired credentials",
            ));
        }
        let credentials = Credentials::new(
            value.access_key_id,
            value.secret_access_key,
            value.session_token,
            value.expiration.map(SystemTime::from),
            "BoundedCredentialProcess",
        );
        *cached = Some(credentials.clone());
        Ok(credentials)
    }
}
impl fmt::Debug for Process {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BoundedCredentialProcess")
    }
}
impl ProvideCredentials for Process {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        future::ProvideCredentials::new(self.credentials())
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Response {
    version: u32,
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
    expiration: Option<chrono::DateTime<chrono::Utc>>,
}

//! Four fixed token audiences share cached native credentials and bounded CLI execution.
use azure_core::credentials::{AccessToken, TokenCredential};
use monitor_integrations::{process::Processes, transport::Error};
use std::{
    ffi::OsStr, fmt, future::Future, io, os::unix::process::ExitStatusExt, pin::Pin, sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;
pub enum Audience {
    Management,
    Logs,
    Registry,
    Vault,
}
impl Audience {
    fn scope(&self) -> &'static str {
        match self {
            Self::Management => "https://management.azure.com/.default",
            Self::Logs => "https://api.loganalytics.io/.default",
            Self::Registry => "https://containerregistry.azure.net/.default",
            Self::Vault => "https://vault.azure.net/.default",
        }
    }
    fn index(&self) -> usize {
        match self {
            Self::Management => 0,
            Self::Logs => 1,
            Self::Registry => 2,
            Self::Vault => 3,
        }
    }
}
pub struct Credential {
    inner: Arc<dyn TokenCredential>,
    cache: [Mutex<Option<AccessToken>>; 4],
}
impl Credential {
    pub fn new(inner: Arc<dyn TokenCredential>) -> Self {
        Self {
            inner,
            cache: std::array::from_fn(|_| Mutex::new(None)),
        }
    }
    pub async fn bearer(&self, audience: Audience) -> Result<String, Error> {
        let mut cached = self.cache[audience.index()].lock().await;
        let now = azure_core::time::OffsetDateTime::now_utc();
        if let Some(token) = cached
            .as_ref()
            .filter(|token| token.expires_on.unix_timestamp() > now.unix_timestamp() + 60)
        {
            return Ok(token.token.secret().into());
        }
        let token = self
            .inner
            .get_token(&[audience.scope()], None)
            .await
            .map_err(|_| Error::Authentication)?;
        if token.expires_on <= now
            || token.token.secret().is_empty()
            || token.token.secret().len() > 65536
        {
            return Err(Error::Authentication);
        }
        let value = token.token.secret().to_owned();
        *cached = Some(token);
        Ok(value)
    }
}
pub struct Executor {
    pub processes: Processes,
    pub timeout: Duration,
    pub limit: usize,
}
impl fmt::Debug for Executor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BoundedAzureExecutor")
    }
}
impl azure_identity::Executor for Executor {
    // The SDK's dynamically dispatched executor requires a boxed future.
    fn run<'s, 'p, 'a, 'v, 'f>(
        &'s self,
        program: &'p OsStr,
        args: &'a [&'v OsStr],
    ) -> Pin<Box<dyn Future<Output = io::Result<std::process::Output>> + Send + 'f>>
    where
        's: 'f,
        'p: 'f,
        'a: 'f,
        'v: 'f,
        Self: 'f,
    {
        Box::pin(async move {
            let mut command = tokio::process::Command::new(program);
            command.args(args);
            let output = self
                .processes
                .run_command(
                    command,
                    self.limit,
                    self.timeout,
                    &tokio_util::sync::CancellationToken::new(),
                )
                .await
                .map_err(|_| io::Error::other("credential helper failed"))?;
            Ok(std::process::Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: output.stdout,
                stderr: output.stderr,
            })
        })
    }
}
#[cfg(test)]
#[path = "azure_auth_tests.rs"]
mod tests;

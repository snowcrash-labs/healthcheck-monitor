//! Fixed helper commands with bounded streams and process-group cleanup.
use super::transport::Error;
use monitor_core::{
    config::types::{Credential, NatsFallback},
    model::Provider,
};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    sync::Semaphore,
};
use tokio_util::sync::CancellationToken;

pub enum Helper {
    #[cfg(test)]
    Fixture {
        script: &'static str,
    },
    #[cfg(test)]
    Missing,
    GithubToken,
    NatsReport {
        context: String,
        fallback: NatsFallback,
    },
    Login {
        credential: Credential,
    },
}
pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
struct Group(i32);
impl Group {
    fn terminate(&mut self) {
        if self.0 > 0 {
            // Kill while the group leader is still owned, before its PID can be reused.
            unsafe {
                libc::kill(-self.0, libc::SIGKILL);
            }
            self.0 = 0;
        }
    }
}
impl Drop for Group {
    fn drop(&mut self) {
        self.terminate();
    }
}
pub struct Processes {
    permits: Semaphore,
}
impl Processes {
    pub fn new(limit: usize) -> Self {
        Self {
            permits: Semaphore::new(limit),
        }
    }
    /// Explicit authentication inherits the foreground terminal so browser/device prompts work.
    pub async fn login(&self, credential: Credential, timeout: Duration) -> Result<(), Error> {
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| Error::Unavailable)?;
        let (executable, args) = command(Helper::Login { credential })?;
        let mut child = Command::new(executable)
            .args(args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| Error::Unavailable)?;
        let outcome = tokio::select! {
            _=tokio::signal::ctrl_c()=>Err(Error::Cancelled),
            result=tokio::time::timeout(timeout,child.wait())=>match result {
                Ok(Ok(status)) if status.success()=>Ok(()),
                Ok(Ok(_))=>Err(Error::Authentication),
                Ok(Err(_))=>Err(Error::Unavailable),
                Err(_)=>Err(Error::Timeout),
            }
        };
        if outcome.is_err() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        outcome
    }
    pub async fn run(
        &self,
        helper: Helper,
        limit: usize,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Output, Error> {
        let permit = tokio::select! { _ = cancel.cancelled() => return Err(Error::Cancelled), permit = self.permits.acquire() => permit.map_err(|_| Error::Unavailable)? };
        let (executable, args) = command(helper)?;
        let mut command = Command::new(executable);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .process_group(0);
        let mut child = command.spawn().map_err(|_| Error::Unavailable)?;
        let mut group = Group(child.id().ok_or(Error::Unavailable)? as i32);
        let stdout = child.stdout.take().ok_or(Error::Unavailable)?;
        let stderr = child.stderr.take().ok_or(Error::Unavailable)?;
        let collect = async {
            let (stdout, stderr, status) =
                tokio::try_join!(read(stdout, limit), read(stderr, limit), async {
                    child.wait().await.map_err(|_| Error::Unavailable)
                })?;
            if !status.success() {
                return Err(Error::Authentication);
            }
            Ok(Output { stdout, stderr })
        };
        let outcome = tokio::select! {
            _ = cancel.cancelled() => Err(Error::Cancelled),
            result = tokio::time::timeout(timeout, collect) => result.map_err(|_| Error::Timeout).and_then(|r| r),
        };
        if outcome.is_err() {
            group.terminate();
            let _ = child.kill().await;
            let _ = child.wait().await;
        } else {
            group.0 = 0;
        }
        drop(permit);
        outcome
    }
}
async fn read<R: AsyncRead + Unpin>(reader: R, limit: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| Error::Unavailable)?;
    if bytes.len() > limit {
        return Err(Error::Limit);
    }
    Ok(bytes)
}
fn command(helper: Helper) -> Result<(&'static str, Vec<String>), Error> {
    match helper {
        #[cfg(test)]
        Helper::Fixture { script } => Ok(("/bin/sh", vec!["-c".into(), script.into()])),
        #[cfg(test)]
        Helper::Missing => Ok(("healthcheck-monitor-missing-fixture", vec![])),
        Helper::GithubToken => Ok(("gh", vec!["auth".into(), "token".into()])),
        Helper::NatsReport { context, fallback } => {
            for value in [&context, &fallback.namespace, &fallback.deployment] {
                if !monitor_core::config::validate::identifier(value) || value.starts_with('-') {
                    return Err(Error::Forbidden);
                }
            }
            Ok((
                "kubectl",
                vec![
                    "--context".into(),
                    context,
                    "-n".into(),
                    fallback.namespace,
                    "exec".into(),
                    format!("deployment/{}", fallback.deployment),
                    "--".into(),
                    "nats".into(),
                    "--server=nats://nats:4222".into(),
                    "--timeout=10s".into(),
                    "stream".into(),
                    "report".into(),
                    "--raw".into(),
                ],
            ))
        }
        Helper::Login { credential } => match credential.provider {
            Provider::Gcp => Ok((
                "gcloud",
                vec!["auth".into(), "application-default".into(), "login".into()],
            )),
            Provider::Aws => Ok((
                "aws",
                vec![
                    "sso".into(),
                    "login".into(),
                    "--profile".into(),
                    credential.profile.ok_or(Error::Authentication)?,
                ],
            )),
            Provider::Azure => Ok((
                "az",
                vec![
                    "login".into(),
                    "--tenant".into(),
                    credential.tenant.ok_or(Error::Authentication)?,
                ],
            )),
            Provider::Github => Ok(("gh", vec!["auth".into(), "login".into(), "--web".into()])),
            _ => Err(Error::Forbidden),
        },
    }
}

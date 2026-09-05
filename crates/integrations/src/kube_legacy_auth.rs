//! Legacy GCP kubeconfig command credentials use the same bounded subprocess runner.
use super::*;
pub async fn acquire(
    provider: &kube::config::AuthProviderConfig,
    processes: &Processes,
    job: &Job,
    cancel: &CancellationToken,
) -> Result<Status, Error> {
    if let Some(token) = provider.config.get("id-token") {
        return status(token, None);
    }
    let expiry = provider
        .config
        .get("expiry")
        .and_then(|expiry| expiry.parse::<DateTime<Utc>>().ok());
    if let Some(token) = provider.config.get("access-token")
        && expiry.is_some_and(|expiry| expiry > Utc::now() + chrono::Duration::seconds(30))
    {
        return status(token, expiry);
    }
    let executable = provider
        .config
        .get("cmd-path")
        .filter(|command| !command.is_empty() && command.len() <= 4096)
        .ok_or(Error::Authentication)?;
    let args = provider
        .config
        .get("cmd-args")
        .map(String::as_str)
        .unwrap_or("");
    if args.len() > 65536 || args.split_whitespace().count() > 128 {
        return Err(Error::Limit);
    }
    let mut command = tokio::process::Command::new(executable);
    command.args(args.split_whitespace());
    for name in provider
        .config
        .get("cmd-drop-env")
        .map(String::as_str)
        .unwrap_or("")
        .split_whitespace()
    {
        command.env_remove(name);
    }
    let output = processes
        .run_command(
            command,
            job.settings.response_bytes.min(65536),
            job.settings.attempt_timeout.duration(),
            cancel,
        )
        .await?;
    if let Some(path) = provider.config.get("token-key") {
        let value: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| Error::Malformed)?;
        let token = field(&value, path)?;
        let expiry = provider
            .config
            .get("expiry-key")
            .map(|path| {
                field(&value, path).and_then(|expiry| {
                    expiry
                        .parse::<DateTime<Utc>>()
                        .map_err(|_| Error::Authentication)
                })
            })
            .transpose()?;
        status(token, expiry)
    } else {
        status(
            std::str::from_utf8(&output.stdout)
                .map_err(|_| Error::Malformed)?
                .trim(),
            None,
        )
    }
}
fn field<'a>(value: &'a serde_json::Value, path: &str) -> Result<&'a str, Error> {
    let path = path
        .trim_matches(['"', '{', '}'])
        .trim_start_matches('$')
        .trim_start_matches('.');
    if path.len() > 512 {
        return Err(Error::Limit);
    }
    let mut selected = value;
    for part in path.split('.') {
        if !part
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        {
            return Err(Error::Authentication);
        }
        selected = selected.get(part).ok_or(Error::Authentication)?;
    }
    selected.as_str().ok_or(Error::Authentication)
}
fn status(token: &str, expiry: Option<DateTime<Utc>>) -> Result<Status, Error> {
    if token.is_empty()
        || token.chars().any(char::is_control)
        || expiry.is_some_and(|until| until <= Utc::now())
    {
        return Err(Error::Authentication);
    }
    Ok(Status {
        token: Some(token.into()),
        expiration_timestamp: expiry,
        client_certificate_data: None,
        client_key_data: None,
    })
}

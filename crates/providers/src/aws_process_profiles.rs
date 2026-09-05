//! SDK profile parsing uses bounded in-memory files and substitutes only the selected process source.
use aws_runtime::env_config::file::{EnvConfigFileKind as Kind, EnvConfigFiles};
use monitor_integrations::transport::Error;
use std::{borrow::Cow, collections::BTreeSet, path::PathBuf};
use tokio::io::AsyncReadExt;
pub struct Prepared {
    pub files: EnvConfigFiles,
    pub selected: String,
    pub command: Option<String>,
}
pub async fn load(selected: Option<&str>) -> Result<Prepared, Error> {
    let config = read("AWS_CONFIG_FILE", "config").await?;
    let credentials = read("AWS_SHARED_CREDENTIALS_FILE", "credentials").await?;
    prepare(config, credentials, selected).await
}
pub async fn prepare(
    config: String,
    credentials: String,
    selected: Option<&str>,
) -> Result<Prepared, Error> {
    if config.len() > 1024 * 1024 || credentials.len() > 1024 * 1024 {
        return Err(Error::Limit);
    }
    let mut builder = EnvConfigFiles::builder()
        .include_default_config_file(false)
        .include_default_credentials_file(false)
        .with_contents(Kind::Config, config)
        .with_contents(Kind::Credentials, credentials);
    let files = builder.clone().build();
    let profiles = aws_config::profile::load(
        &aws_types::os_shim_internal::Fs::real(),
        &aws_types::os_shim_internal::Env::real(),
        &files,
        selected.map(|name| Cow::Owned(name.to_owned())),
    )
    .await
    .map_err(|_| Error::Authentication)?;
    let selected = profiles.selected_profile().to_owned();
    let mut name = selected.as_str();
    let mut seen = BTreeSet::new();
    let mut process = None;
    loop {
        if !seen.insert(name) || seen.len() > 128 {
            break;
        }
        let Some(profile) = profiles.get_profile(name) else {
            break;
        };
        if profile.get("web_identity_token_file").is_some() {
            break;
        }
        if profile.get("role_arn").is_some()
            && let Some(source) = profile.get("source_profile")
            && source != name
        {
            name = source;
            continue;
        }
        if [
            "credential_source",
            "sso_session",
            "sso_start_url",
            "login_session",
        ]
        .iter()
        .any(|field| profile.get(field).is_some())
        {
            break;
        }
        if let Some(command) = profile.get("credential_process") {
            if command.len() > 65536 || name.len() > 512 || name.contains(['[', ']', '\n', '\r']) {
                return Err(Error::Limit);
            }
            let alias = format!(
                "healthcheck-monitor-process-{}",
                &crate::metric_window::id(&serde_json::json!(name))[..16]
            );
            let overlay = if profile.get("role_arn").is_some()
                && profile.get("source_profile") == Some(name)
            {
                if profiles.get_profile(&alias).is_some() {
                    return Err(Error::Authentication);
                }
                format!(
                    "[{name}]\nsource_profile = {alias}\n[{alias}]\ncredential_source = HealthcheckCredentialProcess\n"
                )
            } else {
                format!("[{name}]\ncredential_source = HealthcheckCredentialProcess\n")
            };
            builder = builder.with_contents(Kind::Credentials, overlay);
            process = Some(command.to_owned());
        }
        break;
    }
    Ok(Prepared {
        files: builder.build(),
        selected,
        command: process,
    })
}
async fn read(variable: &str, file: &str) -> Result<String, Error> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let path = std::env::var_os(variable)
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".aws").join(file)));
    let Some(mut path) = path else {
        return Ok(String::new());
    };
    if let Ok(relative) = path.strip_prefix("~")
        && let Some(home) = home
    {
        path = home.join(relative);
    }
    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(_) => return Err(Error::Authentication),
    };
    let mut text = String::new();
    file.take(1024 * 1024 + 1)
        .read_to_string(&mut text)
        .await
        .map_err(|_| Error::Authentication)?;
    if text.len() > 1024 * 1024 {
        return Err(Error::Limit);
    }
    Ok(text)
}

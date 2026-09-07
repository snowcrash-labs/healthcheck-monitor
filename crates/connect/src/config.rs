//! Private connection profiles are kept separate from per-user refresh tokens.
use crate::error::Error;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub server: url::Url,
    pub client_id: String,
    pub client_secret: Option<String>,
    #[serde(default)]
    pub credential_store: Store,
    pub credential_file: Option<PathBuf>,
}
#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Store {
    #[default]
    Keyring,
    File,
}
impl Config {
    pub fn path(path: Option<PathBuf>) -> Result<PathBuf, Error> {
        match path {
            Some(path) => Ok(path),
            None => {
                let root = std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
                    .ok_or(Error::Configuration)?;
                Ok(root.join("healthcheck-connect/config.toml"))
            }
        }
    }
    pub async fn import(path: Option<PathBuf>, source: &std::path::Path) -> Result<(), Error> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        #[derive(Deserialize)]
        struct Download {
            installed: Desktop,
        }
        #[derive(Deserialize)]
        struct Desktop {
            client_id: String,
            client_secret: Option<String>,
        }
        let source = source.to_owned();
        let bytes = tokio::task::spawn_blocking(move || {
            use std::io::Read;
            let file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(source)
                .map_err(|_| Error::Configuration)?;
            if !file.metadata().map_err(|_| Error::Configuration)?.is_file() {
                return Err(Error::Configuration);
            }
            let mut bytes = Vec::new();
            file.take(16385)
                .read_to_end(&mut bytes)
                .map_err(|_| Error::Configuration)?;
            if bytes.len() > 16384 {
                return Err(Error::Configuration);
            }
            Ok(bytes)
        })
        .await
        .map_err(|_| Error::Configuration)??;
        let client: Download = serde_json::from_slice(&bytes).map_err(|_| Error::Configuration)?;
        let config = Self {
            server: "https://health.soundpatrol.com/"
                .parse()
                .map_err(|_| Error::Configuration)?,
            client_id: client.installed.client_id,
            client_secret: client.installed.client_secret,
            credential_store: Store::Keyring,
            credential_file: None,
        };
        config.validate()?;
        let path = Self::path(path)?;
        tokio::task::spawn_blocking(move || {
            use std::io::Write;
            let parent = path.parent().ok_or(Error::Configuration)?;
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|_| Error::Configuration)?;
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                    .map_err(|_| Error::Configuration)?;
            }
            private_parent(&path)?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .map_err(|_| Error::Configuration)?;
            let text = toml::to_string(&config).map_err(|_| Error::Configuration)?;
            file.write_all(text.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|_| Error::Configuration)
        })
        .await
        .map_err(|_| Error::Configuration)??;
        tracing::info!("Desktop client configured; run healthcheck-connect login");
        Ok(())
    }
    pub async fn load(path: Option<PathBuf>) -> Result<Self, Error> {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let path = Self::path(path)?;
        let read_path = path.clone();
        let bytes = tokio::task::spawn_blocking(move || {
            use std::io::Read;
            private_parent(&read_path)?;
            let file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(read_path)
                .map_err(|_| Error::Configuration)?;
            let metadata = file.metadata().map_err(|_| Error::Configuration)?;
            let uid = unsafe { libc::geteuid() };
            if !metadata.is_file()
                || metadata.len() > 16384
                || metadata.mode() & 0o077 != 0
                || (metadata.uid() != uid && metadata.uid() != 0)
            {
                return Err(Error::Configuration);
            }
            let mut bytes = String::new();
            file.take(16385)
                .read_to_string(&mut bytes)
                .map_err(|_| Error::Configuration)?;
            if bytes.len() > 16384 {
                return Err(Error::Configuration);
            }
            Ok(bytes)
        })
        .await
        .map_err(|_| Error::Configuration)??;
        let mut config: Self = toml::from_str(&bytes).map_err(|_| Error::Configuration)?;
        if let Some(file) = &mut config.credential_file
            && file.is_relative()
        {
            *file = path.parent().ok_or(Error::Configuration)?.join(&*file);
        }
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<(), Error> {
        if self.server.scheme() != "https"
            || self.server.host_str().is_none()
            || self.server.path() != "/"
            || !self.server.username().is_empty()
            || self.server.password().is_some()
            || self.server.query().is_some()
            || self.server.fragment().is_some()
            || self.server.port().is_some_and(|p| p != 443)
            || self.client_id.len() > 256
            || !self.client_id.ends_with(".apps.googleusercontent.com")
            || self.client_id.chars().any(char::is_control)
            || self
                .client_secret
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 1024 || s.chars().any(char::is_control))
            || matches!(self.credential_store, Store::File) && self.credential_file.is_none()
        {
            return Err(Error::Configuration);
        }
        Ok(())
    }
}

/// Reject replaceable profiles even when the file itself has private permissions.
fn private_parent(path: &std::path::Path) -> Result<(), Error> {
    use std::os::unix::fs::MetadataExt;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let metadata = std::fs::symlink_metadata(parent).map_err(|_| Error::Configuration)?;
    let uid = unsafe { libc::geteuid() };
    if !metadata.is_dir()
        || metadata.mode() & 0o022 != 0
        || (metadata.uid() != uid && metadata.uid() != 0)
    {
        return Err(Error::Configuration);
    }
    Ok(())
}

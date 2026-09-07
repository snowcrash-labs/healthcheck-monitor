//! Non-secret connection settings are kept separate from per-user credentials.
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
        let meta = tokio::fs::metadata(source)
            .await
            .map_err(|_| Error::Configuration)?;
        if meta.len() > 16384 {
            return Err(Error::Configuration);
        }
        let bytes = tokio::fs::read(source)
            .await
            .map_err(|_| Error::Configuration)?;
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
        let path = match path {
            Some(path) => path,
            None => {
                let root = std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
                    .ok_or(Error::Configuration)?;
                root.join("healthcheck-connect/config.toml")
            }
        };
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|_| Error::Configuration)?;
        if metadata.len() > 16384 {
            return Err(Error::Configuration);
        }
        let bytes = tokio::fs::read_to_string(&path)
            .await
            .map_err(|_| Error::Configuration)?;
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

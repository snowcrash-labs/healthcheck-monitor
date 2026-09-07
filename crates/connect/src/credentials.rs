//! Per-user credentials use the OS store or explicitly chosen atomic owner-only files.
use crate::{
    config::{Config, Store},
    error::Error,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::{
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    sync::Arc,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub refresh_token: String,
    pub email: String,
    pub subject: String,
    pub client_id: String,
    pub server: String,
}
#[derive(Clone)]
pub struct CredentialsStore {
    config: Arc<Config>,
    account: String,
    operations: Arc<tokio::sync::Semaphore>,
}
impl CredentialsStore {
    pub fn new(config: &Config) -> Self {
        let digest = sha2::Sha256::digest(format!("{}|{}", config.server, config.client_id));
        let account = digest.iter().map(|b| format!("{b:02x}")).collect();
        Self {
            config: Arc::new(config.clone()),
            account,
            operations: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }
    pub async fn load(&self) -> Result<Credentials, Error> {
        let permit = self
            .operations
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Credentials)?;
        let store = self.clone();
        let credential = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let text = match store.config.credential_store {
                Store::Keyring => store
                    .entry()?
                    .get_password()
                    .map_err(Error::credential_read)?,
                Store::File => {
                    let path = store
                        .config
                        .credential_file
                        .as_ref()
                        .ok_or(Error::Configuration)?;
                    let file = std::fs::OpenOptions::new()
                        .read(true)
                        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                        .open(path)
                        .map_err(|error| match error.kind() {
                            std::io::ErrorKind::NotFound => Error::LoginRequired,
                            _ => Error::Credentials,
                        })?;
                    let meta = file.metadata().map_err(|_| Error::Credentials)?;
                    // Metadata belongs to the opened descriptor, preventing symlink and rename races.
                    if !meta.is_file()
                        || meta.permissions().mode() & 0o077 != 0
                        || meta.uid() != unsafe { libc::geteuid() }
                        || meta.len() > 16384
                    {
                        return Err(Error::Credentials);
                    }
                    let mut text = String::new();
                    file.take(16385)
                        .read_to_string(&mut text)
                        .map_err(|_| Error::Credentials)?;
                    text
                }
            };
            if text.len() > 16384 {
                return Err(Error::Credentials);
            }
            serde_json::from_str::<Credentials>(&text).map_err(|_| Error::Credentials)
        })
        .await
        .map_err(|_| Error::Credentials)??;
        if credential.client_id != self.config.client_id
            || credential.server != self.config.server.as_str()
            || credential.refresh_token.is_empty()
            || credential.refresh_token.len() > 8192
        {
            return Err(Error::LoginRequired);
        }
        Ok(credential)
    }
    pub async fn save(&self, credential: Credentials) -> Result<(), Error> {
        let permit = self
            .operations
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Credentials)?;
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let text = serde_json::to_string(&credential).map_err(|_| Error::Credentials)?;
            if text.len() > 16384 {
                return Err(Error::Credentials);
            }
            match store.config.credential_store {
                Store::Keyring => store
                    .entry()?
                    .set_password(&text)
                    .map_err(|_| Error::Credentials),
                Store::File => store.write_file(&text),
            }
        })
        .await
        .map_err(|_| Error::Credentials)?
    }
    pub async fn delete(&self) -> Result<(), Error> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || match store.config.credential_store {
            Store::Keyring => store
                .entry()?
                .delete_credential()
                .map_err(|_| Error::Credentials),
            Store::File => std::fs::remove_file(
                store
                    .config
                    .credential_file
                    .as_ref()
                    .ok_or(Error::Configuration)?,
            )
            .map_err(|_| Error::Credentials),
        })
        .await
        .map_err(|_| Error::Credentials)?
    }
    fn entry(&self) -> Result<keyring::Entry, Error> {
        keyring::Entry::new("com.soundpatrol.healthcheck-connect", &self.account)
            .map_err(|_| Error::Credentials)
    }
    fn write_file(&self, text: &str) -> Result<(), Error> {
        use std::io::Write;
        let path = self
            .config
            .credential_file
            .as_ref()
            .ok_or(Error::Configuration)?;
        let parent = path.parent().ok_or(Error::Configuration)?;
        if !parent.exists() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .create(parent)
                .map_err(|_| Error::Credentials)?;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| Error::Credentials)?;
        }
        let metadata = std::fs::symlink_metadata(parent).map_err(|_| Error::Credentials)?;
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o022 != 0
        {
            return Err(Error::Credentials);
        }
        let temporary = parent.join(format!(
            ".healthcheck-{}.tmp",
            oauth2::CsrfToken::new_random().secret()
        ));
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)
                .map_err(|_| Error::Credentials)?;
            file.write_all(text.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|_| Error::Credentials)?;
            std::fs::rename(&temporary, path).map_err(|_| Error::Credentials)?;
            std::fs::File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(|_| Error::Credentials)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

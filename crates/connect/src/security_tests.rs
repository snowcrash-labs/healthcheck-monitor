//! Local trust and revocation regressions use synthetic credentials without network access.
use crate::{
    client::Client,
    config::{Config, Store},
    credentials::Credentials,
    error::Error,
};
use std::os::unix::fs::{PermissionsExt, symlink};

fn config(path: std::path::PathBuf) -> Result<Config, Box<dyn std::error::Error>> {
    Ok(Config {
        server: "https://health.soundpatrol.com/".parse()?,
        client_id: "fixture.apps.googleusercontent.com".into(),
        client_secret: None,
        credential_store: Store::File,
        credential_file: Some(path),
    })
}

#[tokio::test]
async fn profiles_reject_readable_files_symlinks_and_replaceable_parents()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        toml::to_string(&config(dir.path().join("credentials"))?)?,
    )?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    assert!(Config::load(Some(path.clone())).await.is_ok());
    let link = dir.path().join("link.toml");
    symlink(&path, &link)?;
    assert!(Config::load(Some(link)).await.is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))?;
    assert!(Config::load(Some(path.clone())).await.is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o777))?;
    assert!(Config::load(Some(path)).await.is_err());
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[tokio::test]
async fn imports_are_bounded_regular_files_and_never_overwrite()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let source = dir.path().join("desktop.json");
    let target = dir.path().join("config.toml");
    std::fs::write(&source, vec![b' '; 16385])?;
    assert!(Config::import(Some(target.clone()), &source).await.is_err());
    assert!(!target.exists());
    assert!(
        Config::import(Some(target.clone()), dir.path())
            .await
            .is_err()
    );
    std::fs::write(
        &source,
        r#"{"installed":{"client_id":"fixture.apps.googleusercontent.com"}}"#,
    )?;
    let link = dir.path().join("link.json");
    symlink(&source, &link)?;
    assert!(Config::import(Some(target.clone()), &link).await.is_err());
    Config::import(Some(target.clone()), &source).await?;
    let original = std::fs::read(&target)?;
    assert!(Config::import(Some(target.clone()), &source).await.is_err());
    assert_eq!(std::fs::read(&target)?, original);
    assert_eq!(
        std::fs::metadata(&target)?.permissions().mode() & 0o777,
        0o600
    );
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o777))?;
    let unsafe_target = dir.path().join("replaceable.toml");
    assert!(
        Config::import(Some(unsafe_target.clone()), &source)
            .await
            .is_err()
    );
    assert!(!unsafe_target.exists());
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[tokio::test]
async fn failed_revocation_retains_credentials_for_retry() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = tempfile::tempdir()?;
    let config = config(dir.path().join("credentials.json"))?;
    let client = Client::new(config.clone())?;
    client
        .store
        .save(Credentials {
            refresh_token: "synthetic-refresh".into(),
            email: "fixture@soundpatrol.com".into(),
            subject: "fixture".into(),
            client_id: config.client_id,
            server: config.server.to_string(),
        })
        .await?;
    assert!(matches!(
        client.finish_logout(false).await,
        Err(Error::Revocation)
    ));
    assert_eq!(client.store.load().await?.subject, "fixture");
    client.finish_logout(true).await?;
    assert!(matches!(
        client.store.load().await,
        Err(Error::LoginRequired)
    ));
    Ok(())
}

#[tokio::test]
async fn symlink_errors_do_not_request_another_login() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("credentials.json");
    symlink("missing", &path)?;
    let client = Client::new(config(path)?)?;
    assert!(matches!(client.store.load().await, Err(Error::Credentials)));
    Ok(())
}

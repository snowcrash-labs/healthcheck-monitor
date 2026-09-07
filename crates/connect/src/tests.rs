//! Credential and protocol boundary tests require no Google account or monitored target.
use crate::{
    client::Client,
    config::{Config, Store},
    credentials::{Credentials, CredentialsStore},
};
use tokio::io::AsyncReadExt;

fn config(path: std::path::PathBuf) -> Result<Config, Box<dyn std::error::Error>> {
    Ok(Config {
        server: "https://health.soundpatrol.com/".parse()?,
        client_id: "fixture.apps.googleusercontent.com".into(),
        client_secret: None,
        credential_store: Store::File,
        credential_file: Some(path),
    })
}
#[test]
fn callback_rejects_wrong_state_duplicates_and_errors() {
    for input in [
        "GET /callback?state=wrong&code=secret HTTP/1.1\r\n\r\n",
        "GET /callback?state=right&state=right&code=secret HTTP/1.1\r\n\r\n",
        "GET /callback?state=right&code=a&code=b HTTP/1.1\r\n\r\n",
        "GET /callback?state=right&error=denied HTTP/1.1\r\n\r\n",
    ] {
        assert!(crate::login::parse_callback(input.as_bytes(), "right").is_err());
    }
    assert_eq!(
        crate::login::parse_callback(
            b"GET /callback?state=right&code=accepted HTTP/1.1\r\n\r\n",
            "right"
        )
        .ok()
        .as_deref(),
        Some("accepted")
    );
}
#[tokio::test]
async fn writable_connection_profiles_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        toml::to_string(&config(dir.path().join("credentials"))?)?,
    )?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    assert!(Config::load(Some(path.clone())).await.is_ok());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666))?;
    assert!(Config::load(Some(path)).await.is_err());
    Ok(())
}
#[tokio::test]
async fn file_credentials_are_private_and_profile_bound() -> Result<(), Box<dyn std::error::Error>>
{
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("credentials.json");
    let config = config(path.clone())?;
    let store = CredentialsStore::new(&config);
    store
        .save(Credentials {
            refresh_token: "test-only-refresh".into(),
            email: "fixture@soundpatrol.com".into(),
            subject: "fixture".into(),
            client_id: config.client_id.clone(),
            server: config.server.to_string(),
        })
        .await?;
    assert_eq!(
        std::fs::metadata(&path)?.permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(store.load().await?.subject, "fixture");
    let mut other = config.clone();
    other.client_id = "different.apps.googleusercontent.com".into();
    assert!(CredentialsStore::new(&other).load().await.is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))?;
    assert!(store.load().await.is_err());
    Ok(())
}
#[tokio::test]
async fn credential_symlinks_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("link");
    std::os::unix::fs::symlink("missing", &path)?;
    assert!(CredentialsStore::new(&config(path)?).load().await.is_err());
    Ok(())
}
#[tokio::test]
async fn oversized_mcp_frames_fail_before_accumulation() -> Result<(), Box<dyn std::error::Error>> {
    let input = vec![b'x'; 70000];
    let mut reader = crate::input::Limited::new(input.as_slice());
    let mut output = Vec::new();
    assert!(reader.read_to_end(&mut output).await.is_err());
    assert!(output.len() <= 65536);
    Ok(())
}
#[tokio::test]
async fn frame_limit_resets_between_messages() -> Result<(), Box<dyn std::error::Error>> {
    let input = "x".repeat(60000) + "\n" + &"y".repeat(60000) + "\n";
    let mut reader = crate::input::Limited::new(input.as_bytes());
    let mut output = Vec::new();
    reader.read_to_end(&mut output).await?;
    assert_eq!(output.len(), input.len());
    Ok(())
}
#[tokio::test]
async fn mcp_advertises_only_read_only_tools_without_login()
-> Result<(), Box<dyn std::error::Error>> {
    use rmcp::ServiceExt;
    let dir = tempfile::tempdir()?;
    let connector = crate::mcp::Connector::new(Client::new(config(dir.path().join("missing"))?)?);
    let (client, server) = tokio::io::duplex(65536);
    let server = tokio::spawn(async move {
        let running = connector.serve(server).await?;
        running
            .waiting()
            .await
            .map_err(Box::<dyn std::error::Error + Send + Sync>::from)
    });
    let client = ().serve(client).await?;
    let tools = client.list_all_tools().await?;
    assert_eq!(tools.len(), 7);
    assert!(tools.iter().all(|t| {
        t.annotations
            .as_ref()
            .is_some_and(|a| a.read_only_hint == Some(true) && a.destructive_hint == Some(false))
    }));
    assert!(tools.iter().all(|t| t.output_schema.is_some()));
    client.cancel().await?;
    server.await?.map_err(|_| "MCP server failed")?;
    Ok(())
}

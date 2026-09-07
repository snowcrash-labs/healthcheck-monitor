//! Browser authorization uses PKCE and a short-lived loopback callback with bounded input.
use crate::{client::Client, credentials::Credentials, error::Error};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn login(client: &Client) -> Result<(), Error> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| Error::Callback)?;
    let address = listener.local_addr().map_err(|_| Error::Callback)?;
    let redirect = format!("http://127.0.0.1:{}/callback", address.port());
    let state = oauth2::CsrfToken::new_random();
    let nonce = oauth2::CsrfToken::new_random();
    let (challenge, verifier) = oauth2::PkceCodeChallenge::new_random_sha256();
    let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
        .map_err(|_| Error::Configuration)?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", client.config.client_id.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("response_type", "code"),
        ("scope", "openid email"),
        ("access_type", "offline"),
        ("prompt", "consent select_account"),
        ("state", state.secret()),
        ("nonce", nonce.secret()),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
    ]);
    let browser_url = url.to_string();
    tokio::task::spawn_blocking(move || webbrowser::open(&browser_url))
        .await
        .map_err(|_| Error::Callback)?
        .map_err(|_| Error::Callback)?;
    let code = tokio::time::timeout(Duration::from_secs(180), callback(listener, state.secret()))
        .await
        .map_err(|_| Error::Callback)??;
    let mut parameters = vec![
        ("grant_type", "authorization_code"),
        ("client_id", client.config.client_id.as_str()),
        ("code", code.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("code_verifier", verifier.secret()),
    ];
    if let Some(secret) = &client.config.client_secret {
        parameters.push(("client_secret", secret));
    }
    let response = client.exchange(&parameters).await?;
    let token = response.id_token.ok_or(Error::LoginRequired)?;
    let mut auth = client.auth.lock().await;
    let claims = client
        .verify(&token, Some(nonce.secret()), &mut auth.keys)
        .await?;
    client
        .store
        .save(Credentials {
            refresh_token: response.refresh_token.ok_or(Error::LoginRequired)?,
            email: claims.email.clone(),
            subject: claims.sub,
            client_id: client.config.client_id.clone(),
            server: client.config.server.to_string(),
        })
        .await?;
    auth.token = Some((token, claims.exp));
    drop(auth);
    client
        .get::<_, monitor_query::response::Page<monitor_query::response::ScopeInfo>>(
            "scopes",
            &monitor_query::filter::Filter::default(),
        )
        .await?;
    tracing::info!(email=%claims.email,"Google sign-in and monitoring access verified");
    Ok(())
}
async fn callback(listener: tokio::net::TcpListener, state: &str) -> Result<String, Error> {
    let (mut stream, peer) = listener.accept().await.map_err(|_| Error::Callback)?;
    if !peer.ip().is_loopback() {
        return Err(Error::Callback);
    }
    let mut input = Vec::with_capacity(4096);
    let mut chunk = [0u8; 1024];
    loop {
        let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk))
            .await
            .map_err(|_| Error::Callback)?
            .map_err(|_| Error::Callback)?;
        if n == 0 || input.len() + n > 8192 {
            return Err(Error::Callback);
        }
        input.extend_from_slice(&chunk[..n]);
        if input.windows(4).any(|s| s == b"\r\n\r\n") {
            break;
        }
    }
    let code = parse_callback(&input, state)?;
    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nConnection: close\r\n\r\nGoogle authorization received. Return to the terminal.\n").await.map_err(|_|Error::Callback)?;
    Ok(code)
}
pub fn parse_callback(input: &[u8], state: &str) -> Result<String, Error> {
    let input = std::str::from_utf8(input).map_err(|_| Error::Callback)?;
    let line = input.lines().next().ok_or(Error::Callback)?;
    let mut parts = line.split_whitespace();
    if parts.next() != Some("GET") {
        return Err(Error::Callback);
    }
    let path = parts.next().ok_or(Error::Callback)?;
    let url = url::Url::parse(&format!("http://127.0.0.1{path}")).map_err(|_| Error::Callback)?;
    if url.path() != "/callback" {
        return Err(Error::Callback);
    }
    let params: Vec<_> = url.query_pairs().collect();
    if params.iter().filter(|(k, _)| k == "state").count() != 1
        || params.iter().filter(|(k, _)| k == "code").count() != 1
        || !params.iter().any(|(k, v)| k == "state" && v == state)
        || params.iter().any(|(k, _)| k == "error")
    {
        return Err(Error::Callback);
    }
    params
        .into_iter()
        .find(|(k, v)| k == "code" && !v.is_empty() && v.len() <= 4096)
        .map(|(_, v)| v.into_owned())
        .ok_or(Error::Callback)
}

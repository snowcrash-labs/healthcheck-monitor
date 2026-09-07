//! Synthetic Google ID tokens exercise verification without accounts, secrets, or remote requests.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{EncodingKey, Header, encode};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

async fn openssl(
    args: &[&str],
    input: Option<&[u8]>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut child = tokio::process::Command::new("openssl")
        .args(args)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take()
        && let Some(input) = input
    {
        stdin.write_all(input).await?;
    }
    let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output()).await??;
    if !output.status.success() || output.stdout.len() > 8192 {
        return Err("signing fixture failed".into());
    }
    Ok(output.stdout)
}
async fn key() -> Result<(EncodingKey, Keys), Box<dyn std::error::Error>> {
    let pem = openssl(
        &[
            "genpkey",
            "-algorithm",
            "RSA",
            "-pkeyopt",
            "rsa_keygen_bits:2048",
        ],
        None,
    )
    .await?;
    let modulus = openssl(&["rsa", "-modulus", "-noout"], Some(&pem)).await?;
    let text = std::str::from_utf8(&modulus)?
        .trim()
        .split_once('=')
        .ok_or("modulus")?
        .1;
    let bytes = text
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|s| {
            u8::from_str_radix(std::str::from_utf8(s)?, 16)
                .map_err(Box::<dyn std::error::Error>::from)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let jwks = serde_json::from_value(
        json!({"keys":[{"kty":"RSA","kid":"fixture","alg":"RS256","use":"sig","n":URL_SAFE_NO_PAD.encode(bytes),"e":"AQAB"}]}),
    )?;
    let der = openssl(&["pkey", "-outform", "DER"], Some(&pem)).await?;
    Ok((
        EncodingKey::from_rsa_der(&der),
        Keys {
            jwks,
            until: Instant::now() + Duration::from_secs(60),
        },
    ))
}
#[tokio::test]
async fn google_tokens_require_signature_audience_issuer_expiry_nonce_and_corporate_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let client = Client::new(crate::config::Config {
        server: "https://health.soundpatrol.com/".parse()?,
        client_id: "fixture.apps.googleusercontent.com".into(),
        client_secret: None,
        credential_store: crate::config::Store::File,
        credential_file: Some(dir.path().join("unused")),
    })?;
    let (key, keys) = key().await?;
    let mut keys = Some(keys);
    let now = chrono::Utc::now().timestamp();
    let claims = json!({"iss":"https://accounts.google.com","aud":client.config.client_id,"sub":"subject","email":"fixture@soundpatrol.com","email_verified":true,"exp":now+300,"nonce":"nonce"});
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("fixture".into());
    let token = encode(&header, &claims, &key)?;
    assert_eq!(
        client.verify(&token, Some("nonce"), &mut keys).await?.sub,
        "subject"
    );
    for (field, value) in [
        ("iss", json!("https://attacker.invalid")),
        ("aud", json!("other.apps.googleusercontent.com")),
        ("exp", json!(now - 300)),
        ("nonce", json!("different")),
        ("email", json!("outside@example.com")),
        ("email_verified", Value::Bool(false)),
    ] {
        let mut invalid = claims.clone();
        invalid[field] = value;
        assert!(
            client
                .verify(&encode(&header, &invalid, &key)?, Some("nonce"), &mut keys)
                .await
                .is_err()
        );
    }
    let mut damaged = token.into_bytes();
    let at = damaged.len().saturating_sub(10);
    let byte = damaged.get_mut(at).ok_or("token")?;
    *byte = if *byte == b'A' { b'B' } else { b'A' };
    assert!(
        client
            .verify(std::str::from_utf8(&damaged)?, Some("nonce"), &mut keys)
            .await
            .is_err()
    );
    Ok(())
}

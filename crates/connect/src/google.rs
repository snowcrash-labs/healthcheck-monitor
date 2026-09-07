//! Verify Google's ID tokens before retaining identity or sending tokens to IAP.
use crate::{
    client::{Client, decode},
    error::Error,
};
use jsonwebtoken::{
    Algorithm, DecodingKey, Validation, decode as jwt_decode, decode_header, jwk::JwkSet,
};
use serde::Deserialize;
use std::time::{Duration, Instant};

#[derive(Clone, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub email: String,
    email_verified: bool,
    pub exp: u64,
    nonce: Option<String>,
}
#[derive(Deserialize)]
pub struct TokenResponse {
    pub id_token: Option<String>,
    pub refresh_token: Option<String>,
}
pub struct Keys {
    jwks: JwkSet,
    until: Instant,
}
impl Client {
    pub async fn verify(
        &self,
        token: &str,
        nonce: Option<&str>,
        keys: &mut Option<Keys>,
    ) -> Result<Claims, Error> {
        if token.len() > 16384 {
            return Err(Error::Response);
        }
        let header = decode_header(token).map_err(|_| Error::LoginRequired)?;
        if header.alg != Algorithm::RS256 {
            return Err(Error::LoginRequired);
        }
        let kid = header
            .kid
            .filter(|s| s.len() <= 128)
            .ok_or(Error::LoginRequired)?;
        if keys
            .as_ref()
            .is_none_or(|k| Instant::now() >= k.until || k.jwks.find(&kid).is_none())
        {
            let response = self
                .http
                .get("https://www.googleapis.com/oauth2/v3/certs")
                .send()
                .await
                .map_err(|_| Error::Network)?;
            let jwks: JwkSet = decode(response, 65536).await?;
            if jwks.keys.is_empty() || jwks.keys.len() > 16 {
                return Err(Error::Response);
            }
            *keys = Some(Keys {
                jwks,
                until: Instant::now() + Duration::from_secs(3600),
            });
        }
        let jwk = keys
            .as_ref()
            .and_then(|k| k.jwks.find(&kid))
            .ok_or(Error::LoginRequired)?;
        let key = DecodingKey::from_jwk(jwk).map_err(|_| Error::LoginRequired)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[&self.config.client_id]);
        validation.set_issuer(&["https://accounts.google.com", "accounts.google.com"]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.leeway = 30;
        let claims = jwt_decode::<Claims>(token, &key, &validation)
            .map_err(|_| Error::LoginRequired)?
            .claims;
        if !claims.email_verified
            || claims.email.len() > 320
            || !claims
                .email
                .to_ascii_lowercase()
                .ends_with("@soundpatrol.com")
            || claims.sub.is_empty()
            || claims.sub.len() > 256
            || nonce.is_some_and(|n| claims.nonce.as_deref() != Some(n))
        {
            return Err(Error::Forbidden);
        }
        Ok(claims)
    }
    pub async fn exchange(&self, parameters: &[(&str, &str)]) -> Result<TokenResponse, Error> {
        let response = self
            .http
            .post("https://oauth2.googleapis.com/token")
            .form(parameters)
            .send()
            .await
            .map_err(|_| Error::Network)?;
        if response.status() == reqwest::StatusCode::BAD_REQUEST
            || response.status() == reqwest::StatusCode::UNAUTHORIZED
        {
            return Err(Error::LoginRequired);
        }
        decode(response, 32768).await
    }
}

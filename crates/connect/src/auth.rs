//! Serialize refreshes per connection profile; tokens never enter tool responses.
use crate::{client::Client, error::Error, google::Keys};
#[derive(Default)]
pub struct Auth {
    pub token: Option<(String, u64)>,
    pub keys: Option<Keys>,
}
impl Client {
    pub async fn token(&self) -> Result<String, Error> {
        let mut auth = self.auth.lock().await;
        let now = chrono::Utc::now().timestamp().max(0) as u64;
        if let Some((token, expires)) = &auth.token
            && *expires > now + 60
        {
            return Ok(token.clone());
        }
        let mut stored = self.store.load().await?;
        let mut parameters = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", stored.refresh_token.as_str()),
            ("client_id", self.config.client_id.as_str()),
        ];
        if let Some(secret) = &self.config.client_secret {
            parameters.push(("client_secret", secret));
        }
        let response = self.exchange(&parameters).await?;
        let token = response.id_token.ok_or(Error::LoginRequired)?;
        let claims = self.verify(&token, None, &mut auth.keys).await?;
        if claims.sub != stored.subject {
            return Err(Error::LoginRequired);
        }
        if let Some(refresh) = response.refresh_token {
            stored.refresh_token = refresh;
            self.store.save(stored).await?;
        }
        auth.token = Some((token.clone(), claims.exp));
        Ok(token)
    }
}

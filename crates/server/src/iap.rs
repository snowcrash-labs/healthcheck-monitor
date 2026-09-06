//! Validate Google IAP signatures and claims against a bounded, refreshable public-key set.
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

pub struct Iap {
    client: reqwest::Client,
    audience: String,
    domains: Vec<String>,
    keys: Mutex<Keys>,
}
struct Keys {
    values: BTreeMap<String, Arc<DecodingKey>>,
    valid_until: Instant,
    refresh_after: Instant,
}
#[derive(Clone, Deserialize)]
struct Claims {
    iss: String,
    aud: String,
    exp: u64,
    iat: u64,
    sub: String,
    email: String,
}
#[derive(Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}
#[derive(Deserialize)]
struct Jwk {
    kid: String,
    kty: String,
    crv: String,
    x: String,
    y: String,
    alg: Option<String>,
}
impl Iap {
    pub fn new(audience: String, domains: Vec<String>) -> Result<Self, crate::Error> {
        let client = reqwest::Client::builder()
            .https_only(true)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(1)
            .build()
            .map_err(|_| crate::Error::Configuration)?;
        Ok(Self {
            client,
            audience,
            domains,
            keys: Mutex::new(Keys {
                values: BTreeMap::new(),
                valid_until: Instant::now(),
                refresh_after: Instant::now(),
            }),
        })
    }
    pub async fn authorized(&self, token: &str) -> bool {
        if token.len() > 16384 {
            return false;
        }
        let Ok(header) = decode_header(token) else {
            return false;
        };
        if header.alg != Algorithm::ES256 {
            return false;
        }
        let Some(kid) = header.kid.filter(|kid| !kid.is_empty() && kid.len() <= 128) else {
            return false;
        };
        let key = {
            let mut keys = self.keys.lock().await;
            let now = Instant::now();
            if (now >= keys.valid_until || !keys.values.contains_key(&kid))
                && now >= keys.refresh_after
            {
                // Unknown key IDs cannot force an unbounded request stream to Google's endpoint.
                keys.refresh_after = now + Duration::from_secs(60);
                if let Some(replacement) = self.refresh().await {
                    keys.values = replacement;
                    keys.valid_until = Instant::now() + Duration::from_secs(3600);
                }
            }
            if Instant::now() >= keys.valid_until {
                return false;
            }
            keys.values.get(&kid).cloned()
        };
        key.is_some_and(|key| validate(token, &key, &self.audience, &self.domains))
    }
    async fn refresh(&self) -> Option<BTreeMap<String, Arc<DecodingKey>>> {
        let mut response = self
            .client
            .get("https://www.gstatic.com/iap/verify/public_key-jwk")
            .send()
            .await
            .ok()?;
        if !response.status().is_success()
            || response.content_length().is_some_and(|size| size > 65536)
        {
            return None;
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if bytes.len() + chunk.len() > 65536 {
                return None;
            }
            bytes.extend_from_slice(&chunk);
        }
        let jwks: Jwks = serde_json::from_slice(&bytes).ok()?;
        if jwks.keys.is_empty() || jwks.keys.len() > 16 {
            return None;
        }
        let mut keys = BTreeMap::new();
        for key in jwks.keys {
            if key.kty != "EC"
                || key.crv != "P-256"
                || key.kid.is_empty()
                || key.kid.len() > 128
                || key.x.len() > 128
                || key.y.len() > 128
                || key.alg.as_deref().is_some_and(|alg| alg != "ES256")
            {
                return None;
            }
            let decoding = DecodingKey::from_ec_components(&key.x, &key.y).ok()?;
            if keys.insert(key.kid, Arc::new(decoding)).is_some() {
                return None;
            }
        }
        Some(keys)
    }
}
fn validate(token: &str, key: &DecodingKey, audience: &str, domains: &[String]) -> bool {
    let mut validation = Validation::new(Algorithm::ES256);
    validation.set_audience(&[audience]);
    validation.set_issuer(&["https://cloud.google.com/iap"]);
    validation.set_required_spec_claims(&["exp", "iat", "iss", "aud", "sub"]);
    validation.validate_nbf = true;
    validation.leeway = 30;
    let Ok(decoded) = decode::<Claims>(token, key, &validation) else {
        return false;
    };
    let claims = decoded.claims;
    let now = chrono::Utc::now().timestamp().max(0) as u64;
    claims.iss == "https://cloud.google.com/iap"
        && claims.aud == audience
        && claims.iat <= now + 30
        && claims.exp > claims.iat
        && claims.exp - claims.iat <= 660
        && !claims.sub.is_empty()
        && claims.sub.len() <= 512
        && claims.email.len() <= 320
        && !claims.email.chars().any(char::is_control)
        && claims
            .email
            .rsplit_once('@')
            .is_some_and(|(local, domain)| {
                !local.is_empty()
                    && domains
                        .iter()
                        .any(|allowed| allowed.eq_ignore_ascii_case(domain))
            })
}
/// Global external Application Load Balancer health checks and backend requests use these ranges.
pub fn trusted_peer(peer: std::net::IpAddr) -> bool {
    match peer {
        std::net::IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            (a == 35 && b == 191) || (a == 130 && b == 211 && c <= 3)
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "iap_tests.rs"]
mod tests;

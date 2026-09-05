//! Verified proxy identity and host checks protect both API data and static application routes.
use crate::{
    api::App,
    config::{Access, Config},
    listener::Peer,
    response::ApiError,
};
use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderValue, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sha2::Digest;
use std::sync::Arc;
pub struct Security {
    access: Access,
    secret: Option<[u8; 32]>,
}
impl Security {
    pub fn new(
        config: &Config,
        secret: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, crate::Error> {
        let digest = match &config.access {
            Access::Local => None,
            Access::Proxy { secret_env, .. } => {
                let value = secret(secret_env)
                    .filter(|value| value.len() >= 32 && value.len() <= 4096)
                    .ok_or(crate::Error::Configuration)?;
                Some(sha2::Sha256::digest(value.as_bytes()).into())
            }
        };
        Ok(Self {
            access: config.access.clone(),
            secret: digest,
        })
    }
    fn authorized(&self, request: &Request, peer: std::net::IpAddr) -> bool {
        let host = request
            .uri()
            .authority()
            .map(|authority| authority.as_str())
            .or_else(|| {
                request
                    .headers()
                    .get(header::HOST)
                    .and_then(|value| value.to_str().ok())
            });
        let Some(host) = host
            .and_then(|host| url::Url::parse(&format!("http://{host}")).ok())
            .and_then(|url| url.host_str().map(String::from))
        else {
            return false;
        };
        if request.uri().path().starts_with("/api/")
            && request
                .headers()
                .get("sec-fetch-site")
                .and_then(|value| value.to_str().ok())
                == Some("cross-site")
        {
            return false;
        }
        match &self.access {
            Access::Local => {
                peer.is_loopback() && matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]")
            }
            Access::Proxy {
                public_origin,
                trusted_peers,
                allowed_domains,
                ..
            } => {
                if Some(host.as_str()) != public_origin.host_str()
                    || !trusted_peers.iter().any(|network| network.contains(&peer))
                {
                    return false;
                }
                let Some(token) = request.headers().get("x-healthcheck-proxy-key") else {
                    return false;
                };
                let digest: [u8; 32] = sha2::Sha256::digest(token.as_bytes()).into();
                if !self.secret.as_ref().is_some_and(|secret| {
                    secret
                        .iter()
                        .zip(digest)
                        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
                        == 0
                }) {
                    return false;
                }
                request
                    .headers()
                    .get("x-auth-request-email")
                    .and_then(|value| value.to_str().ok())
                    .filter(|email| email.len() <= 320)
                    .and_then(|email| email.rsplit_once('@'))
                    .is_some_and(|(local, domain)| {
                        !local.is_empty()
                            && allowed_domains
                                .iter()
                                .any(|allowed| allowed.eq_ignore_ascii_case(domain))
                    })
            }
        }
    }
}
pub async fn guard(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<Peer>>()
        .map(|peer| peer.0.0.ip());
    if request.uri().to_string().len() > 8192 {
        return ApiError::BadQuery.into_response();
    }
    let authorized = request.uri().path() == "/healthz"
        || peer.is_some_and(|peer| app.security.authorized(&request, peer));
    if !authorized {
        return ApiError::Unauthorized.into_response();
    }
    if !matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD
    ) {
        return axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let permit = if request.uri().path() == "/api/v1/events" {
        None
    } else {
        match app.requests.clone().try_acquire_owned() {
            Ok(permit) => Some(permit),
            Err(_) => return ApiError::Busy.into_response(),
        }
    };
    let asset = request.uri().path().starts_with("/assets/");
    let mut response = next.run(request).await;
    for (name, value) in [
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
        ),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
        (
            "permissions-policy",
            "camera=(), microphone=(), geolocation=()",
        ),
        ("x-frame-options", "DENY"),
    ] {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
    if app.tls {
        response.headers_mut().insert(
            "strict-transport-security",
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    if !response.headers().contains_key(header::CACHE_CONTROL) {
        let immutable = asset && response.status().is_success();
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(if immutable {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            }),
        );
    }
    if let Some(permit) = permit {
        let (parts, body) = response.into_parts();
        response = Response::from_parts(
            parts,
            axum::body::Body::new(crate::body::Guarded::new(body, permit)),
        );
    }
    response
}

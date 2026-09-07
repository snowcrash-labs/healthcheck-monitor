//! Listener and access settings are separate from reloadable monitoring configuration.
use serde::Deserialize;
use std::{net::SocketAddr, path::PathBuf};
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub listen: SocketAddr,
    pub tls: Option<Tls>,
    pub access: Access,
    pub history: monitor_history::config::Config,
    pub costs: monitor_costs::config::Config,
    pub connections: usize,
    pub requests: usize,
    pub event_streams: usize,
    pub response_bytes: usize,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tls {
    pub certificate: PathBuf,
    pub private_key: PathBuf,
}
#[derive(Clone, Default, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Access {
    #[default]
    Local,
    Proxy {
        secret_env: String,
        public_origin: url::Url,
        trusted_peers: Vec<ipnet::IpNet>,
        allowed_domains: Vec<String>,
    },
    Iap {
        audience: String,
        public_origin: url::Url,
        allowed_domains: Vec<String>,
    },
}
impl Default for Config {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([127, 0, 0, 1], 9840)),
            tls: None,
            access: Access::Local,
            history: Default::default(),
            costs: Default::default(),
            connections: 64,
            requests: 16,
            event_streams: 16,
            response_bytes: 2 * 1024 * 1024,
        }
    }
}
impl Config {
    pub fn load(path: Option<&std::path::Path>) -> Result<Self, crate::Error> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        use std::io::Read;
        let mut text = String::new();
        std::fs::File::open(path)?
            .take(65537)
            .read_to_string(&mut text)?;
        if text.len() > 65536 {
            return Err(crate::Error::Configuration);
        }
        let mut config: Self = toml::from_str(&text).map_err(|_| crate::Error::Configuration)?;
        if let Some(base) = path.parent()
            && let Some(tls) = &mut config.tls
        {
            if tls.certificate.is_relative() {
                tls.certificate = base.join(&tls.certificate);
            }
            if tls.private_key.is_relative() {
                tls.private_key = base.join(&tls.private_key);
            }
        }
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<(), crate::Error> {
        self.costs
            .validate()
            .map_err(|_| crate::Error::Configuration)?;
        self.history
            .validate()
            .map_err(|_| crate::Error::Configuration)?;
        if !(1..=256).contains(&self.connections)
            || !(1..=64).contains(&self.requests)
            || self.requests > self.connections
            || !(1..=64).contains(&self.event_streams)
            || self.event_streams > self.connections
            || !(1024..=4 * 1024 * 1024).contains(&self.response_bytes)
        {
            return Err(crate::Error::Configuration);
        }
        match &self.access {
            Access::Local if !self.listen.ip().is_loopback() => {
                return Err(crate::Error::Configuration);
            }
            Access::Iap {
                audience,
                public_origin,
                allowed_domains,
            } => {
                let valid_audience = audience
                    .strip_prefix("/projects/")
                    .and_then(|value| value.split_once("/global/backendServices/"))
                    .is_some_and(|(project, service)| {
                        [project, service].iter().all(|value| {
                            !value.is_empty()
                                && value.len() <= 20
                                && value.bytes().all(|byte| byte.is_ascii_digit())
                        })
                    });
                if !valid_audience || !origin(public_origin) || !domains(allowed_domains) {
                    return Err(crate::Error::Configuration);
                }
            }
            Access::Proxy {
                secret_env,
                public_origin,
                trusted_peers,
                allowed_domains,
            } if secret_env.is_empty()
                || secret_env.len() > 128
                || !secret_env.bytes().all(|byte| {
                    byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                })
                || public_origin.scheme() != "https"
                || public_origin.host_str().is_none()
                || !public_origin.username().is_empty()
                || public_origin.password().is_some()
                || public_origin.query().is_some()
                || public_origin.fragment().is_some()
                || public_origin.path() != "/"
                || trusted_peers.is_empty()
                || trusted_peers.len() > 32
                || allowed_domains.is_empty()
                || allowed_domains.len() > 32
                || allowed_domains.iter().any(|domain| {
                    domain.is_empty()
                        || domain.len() > 253
                        || !domain
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b".-".contains(&byte))
                }) =>
            {
                return Err(crate::Error::Configuration);
            }
            _ => {}
        }
        Ok(())
    }
}
fn origin(value: &url::Url) -> bool {
    value.scheme() == "https"
        && value.host_str().is_some()
        && value.username().is_empty()
        && value.password().is_none()
        && value.query().is_none()
        && value.fragment().is_none()
        && value.path() == "/"
}
fn domains(values: &[String]) -> bool {
    !values.is_empty()
        && values.len() <= 32
        && values.iter().all(|value| {
            !value.is_empty()
                && value.len() <= 253
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b".-".contains(&byte))
        })
}

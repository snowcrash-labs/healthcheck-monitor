//! DNS, trusted TLS, expiry, and HTTP status probes never read response bodies.
use super::{
    projection::observation,
    transport::{Error, Http},
};
use chrono::{DateTime, Utc};
use monitor_core::{
    config::{resolve::Job, types::Endpoint},
    model::{Data, Observation},
};
use std::{sync::Arc, time::Instant};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_util::sync::CancellationToken;

pub async fn probe(
    http: &Http,
    job: &Job,
    endpoint: &Endpoint,
    cancel: &CancellationToken,
) -> Observation {
    let start = Instant::now();
    let host = endpoint.url.host_str().unwrap_or("");
    let port = endpoint.url.port_or_known_default().unwrap_or(443);
    let resolver = hickory_resolver::TokioResolver::builder_tokio().and_then(|b| b.build());
    let dns = match resolver {
        Ok(resolver) => tokio::time::timeout(
            job.settings.connect_timeout.duration(),
            resolver.lookup_ip(host),
        )
        .await
        .is_ok_and(|r| r.is_ok_and(|addresses| addresses.iter().next().is_some())),
        Err(_) => false,
    };
    let tls = tokio::select! {
        _ = cancel.cancelled() => Err(Error::Cancelled),
        result = tokio::time::timeout(job.settings.attempt_timeout.duration(), certificate(host, port)) => result.map_err(|_| Error::Timeout).and_then(|r| r),
    };
    let status = tokio::select! {
        _ = cancel.cancelled() => None,
        result = http.client().get(endpoint.url.clone()).send() => result.ok().map(|r| r.status().as_u16()),
    };
    let valid_tls = tls.is_ok();
    let expires_at = tls.ok().flatten();
    observation(
        job,
        "endpoints",
        &endpoint.name,
        Data::Endpoint {
            dns,
            tls: valid_tls,
            status,
            accepted: endpoint.accepted.clone(),
            latency_ms: start.elapsed().as_millis().min(u64::MAX as u128) as u64,
            expires_at,
        },
    )
}
async fn certificate(host: &str, port: u16) -> Result<Option<DateTime<Utc>>, Error> {
    use rustls_platform_verifier::ConfigVerifierExt;
    let config = rustls::ClientConfig::with_platform_verifier().map_err(|_| Error::Unavailable)?;
    let connector = TlsConnector::from(Arc::new(config));
    let name =
        rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|_| Error::Malformed)?;
    let tcp = TcpStream::connect((host, port))
        .await
        .map_err(|_| Error::Unavailable)?;
    let stream = connector
        .connect(name, tcp)
        .await
        .map_err(|_| Error::Unavailable)?;
    let certificate = stream
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|c| c.first())
        .ok_or(Error::Malformed)?;
    let (_, cert) =
        x509_parser::parse_x509_certificate(certificate.as_ref()).map_err(|_| Error::Malformed)?;
    Ok(DateTime::from_timestamp(
        cert.validity().not_after.timestamp(),
        0,
    ))
}

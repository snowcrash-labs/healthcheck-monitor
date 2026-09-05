//! DNS, trusted TLS, expiry, and HTTP status from one request without response bodies.
use super::{
    projection::observation,
    transport::{Error, Http},
};
use chrono::DateTime;
use monitor_core::{
    config::{resolve::Job, types::Endpoint},
    model::{Data, Observation},
};
use std::time::Instant;
use tokio_util::sync::CancellationToken;

pub async fn probe(
    http: &Http,
    job: &Job,
    endpoint: &Endpoint,
    cancel: &CancellationToken,
) -> Observation {
    let mut start = Instant::now();
    let mut dns = false;
    let mut tls = false;
    let mut status = None;
    let mut expires_at = None;
    let work = async {
        let _permit = crate::admission::acquire().await?;
        start = Instant::now();
        let host = endpoint.url.host_str().ok_or(Error::Malformed)?;
        dns = tokio::time::timeout(
            job.settings.connect_timeout.duration(),
            http.pools.network.lookup(host),
        )
        .await
        .is_ok_and(|result| result.is_ok_and(|addresses| addresses.iter().next().is_some()));
        let response = http
            .client_for(&job.settings)?
            .get(endpoint.url.clone())
            .timeout(job.settings.attempt_timeout.duration())
            .send()
            .await
            .map_err(|_| Error::Unavailable)?;
        status = Some(response.status().as_u16());
        // Trust and hostname are checked on the connection that produced the status.
        // Pooled connections retain the leaf certificate, avoiding a second handshake.
        if let Some(certificate) = response
            .extensions()
            .get::<reqwest::tls::TlsInfo>()
            .and_then(|info| info.peer_certificate())
        {
            let (_, certificate) =
                x509_parser::parse_x509_certificate(certificate).map_err(|_| Error::Malformed)?;
            tls = true;
            expires_at = DateTime::from_timestamp(certificate.validity().not_after.timestamp(), 0);
        }
        drop(response);
        Ok::<_, Error>(())
    };
    tokio::select! {
        _ = cancel.cancelled() => {},
        _ = tokio::time::timeout(job.settings.operation_timeout.duration(), work) => {},
    }
    observation(
        job,
        "endpoints",
        &endpoint.name,
        Data::Endpoint {
            dns,
            tls,
            status,
            accepted: endpoint.accepted.clone(),
            latency_ms: start.elapsed().as_millis().min(u64::MAX as u128) as u64,
            expires_at,
        },
    )
}

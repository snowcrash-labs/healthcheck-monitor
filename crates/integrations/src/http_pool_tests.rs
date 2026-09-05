//! Real local TLS proves certificate inspection and cross-target HTTP connection reuse.
use super::*;
use crate::transport::Http;
use monitor_core::{
    config::{
        resolve::Job,
        types::{Config, Endpoint},
    },
    model::Data,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

fn job() -> Result<Job, Box<dyn std::error::Error>> {
    Config::parse("version=1\n[[targets]]\nname='edge'\nprovider='edge'\nscope='fixture'")?
        .resolve(&Default::default())?
        .jobs
        .into_iter()
        .find(|job| job.check == monitor_core::model::Check::Edge)
        .ok_or_else(|| "job".into())
}
#[tokio::test]
async fn pooled_probes_reuse_one_tls_connection_and_never_read_application_bodies()
-> Result<(), Box<dyn std::error::Error>> {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    let key = rustls::pki_types::PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der());
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certified.cert.der().clone()], key.into())?;
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await?;
        let mut stream = acceptor.accept(tcp).await?;
        for index in 0..3 {
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") && headers.len() < 4096 {
                headers.push(stream.read_u8().await?);
            }
            let reply: &[u8] = if index < 2 {
                b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"
            } else {
                b"HTTP/1.1 503 Unavailable\r\nContent-Length: 1000000\r\n\r\n"
            };
            stream.write_all(reply).await?;
            stream.flush().await?;
        }
        // No body is transmitted; a probe attempting to read it would wait until cancellation.
        tokio::time::sleep(Duration::from_secs(5)).await;
        Ok::<_, std::io::Error>(())
    });
    let job = job()?;
    let pools = Arc::new(Pools::default());
    let client = reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .tls_info(true)
        .add_root_certificate(reqwest::Certificate::from_der(certified.cert.der())?)
        .resolve("localhost", address)
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    pools
        .clients
        .entry_sync((job.settings.connect_timeout.0, job.settings.concurrency))
        .or_put(client);
    let first = Http::shared(pools.clone(), &job.settings)?;
    let second = Http::shared(pools, &job.settings)?;
    let endpoint = Endpoint {
        name: "local".into(),
        url: format!("https://localhost:{}/", address.port()).parse()?,
        accepted: vec![200],
    };
    for (index, http) in [&first, &second, &first].into_iter().enumerate() {
        let observation = tokio::time::timeout(
            Duration::from_secs(2),
            crate::endpoint::probe(http, &job, &endpoint, &CancellationToken::new()),
        )
        .await?;
        assert!(
            matches!(observation.data,Data::Endpoint { dns: true, tls: true, expires_at: Some(_), status: Some(status), .. } if status == if index < 2 { 200 } else { 503 })
        );
    }
    server.abort();
    assert!(server.await.is_err_and(|error| error.is_cancelled()));
    Ok(())
}
#[tokio::test]
async fn connection_settings_select_separate_pools_but_attempt_timeouts_do_not()
-> Result<(), Box<dyn std::error::Error>> {
    let pools = Pools::default();
    let mut settings = Settings::default();
    let _ = pools.client(&settings)?;
    settings.attempt_timeout.0 += 10;
    let _ = pools.client(&settings)?;
    assert_eq!(pools.clients.len(), 1);
    settings.connect_timeout.0 += 1;
    let _ = pools.client(&settings)?;
    assert_eq!(pools.clients.len(), 2);
    Ok(())
}

//! Accepted connections and TLS handshakes remain bounded; ALPN prefers HTTP/2.
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use std::{
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore},
};
pub struct Listener {
    inner: TcpListener,
    permits: Arc<Semaphore>,
    tls: Option<tokio_rustls::TlsAcceptor>,
    handshakes: tokio::task::JoinSet<Option<(Stream, SocketAddr)>>,
}
pub struct Stream {
    inner: Transport,
    _permit: OwnedSemaphorePermit,
}
enum Transport {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::server::TlsStream<TcpStream>>),
}
#[derive(Clone, Copy)]
pub struct Peer(pub SocketAddr);
impl axum::extract::connect_info::Connected<axum::serve::IncomingStream<'_, Listener>> for Peer {
    fn connect_info(stream: axum::serve::IncomingStream<'_, Listener>) -> Self {
        Self(*stream.remote_addr())
    }
}
impl Listener {
    pub async fn bind(config: &crate::config::Config) -> Result<Self, crate::Error> {
        let tls = if let Some(tls) = &config.tls {
            let certificate = bounded_file(&tls.certificate)?;
            let key = bounded_file(&tls.private_key)?;
            let certificates = CertificateDer::pem_slice_iter(&certificate)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| crate::Error::Configuration)?;
            let key =
                PrivateKeyDer::from_pem_slice(&key).map_err(|_| crate::Error::Configuration)?;
            let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::aws_lc_rs::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .map_err(|_| crate::Error::Configuration)?
            .with_no_client_auth()
            .with_single_cert(certificates, key)
            .map_err(|_| crate::Error::Configuration)?;
            tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
            Some(tokio_rustls::TlsAcceptor::from(Arc::new(tls)))
        } else {
            None
        };
        Ok(Self {
            inner: TcpListener::bind(config.listen).await?,
            permits: Arc::new(Semaphore::new(config.connections)),
            tls,
            handshakes: tokio::task::JoinSet::new(),
        })
    }
    pub fn address(&self) -> std::io::Result<SocketAddr> {
        self.inner.local_addr()
    }
}
impl axum::serve::Listener for Listener {
    type Io = Stream;
    type Addr = SocketAddr;
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let accept = async {
                let permit = self
                    .permits
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| std::io::Error::other("listener closed"))?;
                let (stream, address) = self.inner.accept().await?;
                Ok::<_, std::io::Error>((stream, address, permit))
            };
            let (stream, address, permit) = tokio::select! {
                finished=self.handshakes.join_next(),if !self.handshakes.is_empty()=>{if let Some(Ok(Some(ready)))=finished{return ready;}continue;},
                result=accept=>match result {Ok(value)=>value,Err(_)=>{tokio::time::sleep(std::time::Duration::from_millis(100)).await;continue;}},
            };
            let _ = stream.set_nodelay(true);
            if let Some(tls) = self.tls.clone() {
                self.handshakes.spawn(async move {
                    let stream = match tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        tls.accept(stream),
                    )
                    .await
                    {
                        Ok(Ok(stream)) => stream,
                        _ => return None,
                    };
                    Some((
                        Stream {
                            inner: Transport::Tls(Box::new(stream)),
                            _permit: permit,
                        },
                        address,
                    ))
                });
            } else {
                return (
                    Stream {
                        inner: Transport::Plain(stream),
                        _permit: permit,
                    },
                    address,
                );
            }
        }
    }
    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}
impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match &mut self.inner {
            Transport::Plain(stream) => Pin::new(stream).poll_read(context, buffer),
            Transport::Tls(stream) => Pin::new(stream.as_mut()).poll_read(context, buffer),
        }
    }
}
impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match &mut self.inner {
            Transport::Plain(stream) => Pin::new(stream).poll_write(context, buffer),
            Transport::Tls(stream) => Pin::new(stream.as_mut()).poll_write(context, buffer),
        }
    }
    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        match &mut self.inner {
            Transport::Plain(stream) => Pin::new(stream).poll_flush(context),
            Transport::Tls(stream) => Pin::new(stream.as_mut()).poll_flush(context),
        }
    }
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        match &mut self.inner {
            Transport::Plain(stream) => Pin::new(stream).poll_shutdown(context),
            Transport::Tls(stream) => Pin::new(stream.as_mut()).poll_shutdown(context),
        }
    }
}
fn bounded_file(path: &std::path::Path) -> Result<Vec<u8>, crate::Error> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(crate::Error::Configuration);
    }
    Ok(bytes)
}

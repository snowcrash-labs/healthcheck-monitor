//! One bounded DNS cache serves HTTP clients and explicit endpoint observations.
use crate::transport::Error;
use std::{net::SocketAddr, sync::Arc};
#[derive(Clone, Default)]
pub struct Network {
    resolver: Arc<tokio::sync::OnceCell<hickory_resolver::TokioResolver>>,
}
impl Network {
    pub async fn lookup(&self, host: &str) -> Result<hickory_resolver::lookup_ip::LookupIp, Error> {
        let resolver = self
            .resolver
            .get_or_try_init(|| async {
                let mut builder = hickory_resolver::TokioResolver::builder_tokio()
                    .map_err(|_| Error::Unavailable)?;
                builder.options_mut().cache_size = 1024;
                builder.build().map_err(|_| Error::Unavailable)
            })
            .await?;
        resolver
            .lookup_ip(host)
            .await
            .map_err(|_| Error::Unavailable)
    }
}
impl reqwest::dns::Resolve for Network {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let network = self.clone();
        Box::pin(async move {
            let addresses = network.lookup(name.as_str()).await?;
            let addresses: Vec<_> = addresses.iter().map(|ip| SocketAddr::new(ip, 0)).collect();
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

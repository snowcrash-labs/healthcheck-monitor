//! Native PostgreSQL connections use rustls and embedded migrations without linking libpq.
use crate::{config::Config, error::Error};
use diesel_async::{
    AsyncPgConnection,
    pooled_connection::{AsyncDieselConnectionManager, ManagerConfig},
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use futures::FutureExt;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
pub type Pool = bb8::Pool<AsyncDieselConnectionManager<AsyncPgConnection>>;
const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

pub struct History {
    pub(crate) pool: Pool,
    pub(crate) read_pool: Pool,
    pub(crate) cost_pool: Pool,
    pub(crate) cost_read_pool: Pool,
    pub(crate) config: Config,
    url: String,
    ready: AtomicBool,
}
impl History {
    pub fn new(url: String, config: Config) -> Result<Arc<Self>, Error> {
        config.validate()?;
        let parsed: tokio_postgres::Config = url.parse().map_err(|_| Error::Configuration)?;
        if parsed.get_dbname().is_none() {
            return Err(Error::Configuration);
        }
        let mut options = ManagerConfig::default();
        options.custom_setup = Box::new(|url| connect(url).boxed());
        let manager = AsyncDieselConnectionManager::new_with_config(url.clone(), options);
        let pool = Pool::builder()
            .max_size(1)
            .min_idle(Some(0))
            .test_on_check_out(true)
            .connection_timeout(Duration::from_secs(5))
            .idle_timeout(Some(Duration::from_secs(60)))
            .max_lifetime(Some(Duration::from_secs(900)))
            .build_unchecked(manager);
        let mut read_options = ManagerConfig::default();
        read_options.custom_setup = Box::new(|url| connect(url).boxed());
        let read_pool = Pool::builder()
            .max_size(config.connections.saturating_sub(1).max(1))
            .min_idle(Some(0))
            .test_on_check_out(true)
            .connection_timeout(Duration::from_secs(5))
            .idle_timeout(Some(Duration::from_secs(60)))
            .max_lifetime(Some(Duration::from_secs(900)))
            .build_unchecked(AsyncDieselConnectionManager::new_with_config(
                url.clone(),
                read_options,
            ));
        let cost_pool = billing_pool(&url);
        let cost_read_pool = billing_pool(&url);
        Ok(Arc::new(Self {
            cost_pool,
            cost_read_pool,
            pool,
            read_pool,
            config,
            url,
            ready: AtomicBool::new(false),
        }))
    }
    pub fn ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
    pub async fn migrate(&self) -> Result<(), Error> {
        let connection = tokio::time::timeout(Duration::from_secs(10), connect(&self.url))
            .await
            .map_err(|_| Error::Connection)?
            .map_err(|_| Error::Connection)?;
        tokio::task::spawn_blocking(move || {
            let mut connection: diesel_async::async_connection_wrapper::AsyncConnectionWrapper<
                AsyncPgConnection,
            > = connection.into();
            connection
                .run_pending_migrations(MIGRATIONS)
                .map(|_| ())
                .map_err(|_| Error::Migration)
        })
        .await
        .map_err(|_| Error::Task)??;
        self.ready.store(true, Ordering::Release);
        Ok(())
    }
}
fn billing_pool(url: &str) -> Pool {
    let mut options = ManagerConfig::default();
    options.custom_setup = Box::new(|url| connect(url).boxed());
    Pool::builder()
        .max_size(1)
        .min_idle(Some(0))
        .test_on_check_out(true)
        .connection_timeout(Duration::from_secs(5))
        .idle_timeout(Some(Duration::from_secs(60)))
        .max_lifetime(Some(Duration::from_secs(900)))
        .build_unchecked(AsyncDieselConnectionManager::new_with_config(
            url.to_owned(),
            options,
        ))
}
async fn connect(url: &str) -> diesel::ConnectionResult<AsyncPgConnection> {
    use rustls_platform_verifier::BuilderVerifierExt;
    let fail = || diesel::ConnectionError::BadConnection("history connection unavailable".into());
    let mut config: tokio_postgres::Config = url.parse().map_err(|_| fail())?;
    config.application_name("soundpatrol-healthcheck-monitor").connect_timeout(Duration::from_secs(5))
        .keepalives_idle(Duration::from_secs(60)).options("-c statement_timeout=5000 -c lock_timeout=2000 -c idle_in_transaction_session_timeout=10000");
    let local = config.get_hosts().iter().all(|host| match host {
        tokio_postgres::config::Host::Unix(_) => true,
        tokio_postgres::config::Host::Tcp(host) => {
            host == "localhost"
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        }
    });
    if !local {
        config.ssl_mode(tokio_postgres::config::SslMode::Require);
    }
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| fail())?
    .with_platform_verifier()
    .map_err(|_| fail())?
    .with_no_client_auth();
    let (client, connection) = config
        .connect(tokio_postgres_rustls::MakeRustlsConnect::new(tls))
        .await
        .map_err(|_| fail())?;
    let mut connection =
        AsyncPgConnection::try_from_client_and_connection(client, connection).await?;
    // The released driver exposes only unlimited or disabled statement caching.
    // Dynamic query shapes must not create a runtime-growing cache.
    use diesel_async::AsyncConnection;
    connection.set_prepared_statement_cache_size(diesel::connection::CacheSize::Disabled);
    Ok(connection)
}

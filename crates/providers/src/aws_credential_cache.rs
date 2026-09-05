//! Native SDK calls and signed REST requests share one refreshable credential generation.
use aws_credential_types::{
    Credentials,
    provider::{self, ProvideCredentials, SharedCredentialsProvider, future},
};
use std::{
    fmt,
    time::{Duration, SystemTime},
};
use tokio::sync::Mutex;
pub struct Cached {
    inner: SharedCredentialsProvider,
    value: Mutex<Option<Credentials>>,
}
impl Cached {
    pub fn new(inner: SharedCredentialsProvider) -> Self {
        Self {
            inner,
            value: Mutex::new(None),
        }
    }
    async fn credentials(&self) -> provider::Result {
        let mut cached = self.value.lock().await;
        let now = SystemTime::now();
        if let Some(credentials) = cached.as_ref().filter(|credentials| {
            credentials
                .expiry()
                .is_none_or(|expiry| expiry > now + Duration::from_secs(60))
        }) {
            return Ok(credentials.clone());
        }
        let credentials = self.inner.provide_credentials().await?;
        if credentials.expiry().is_some_and(|expiry| expiry <= now) {
            return Err(
                aws_credential_types::provider::error::CredentialsError::provider_error(
                    "credentials are expired",
                ),
            );
        }
        *cached = Some(credentials.clone());
        Ok(credentials)
    }
}
impl fmt::Debug for Cached {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CachedNativeCredentials")
    }
}
impl ProvideCredentials for Cached {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        future::ProvideCredentials::new(self.credentials())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    #[derive(Debug)]
    struct Source(Arc<AtomicUsize>);
    impl ProvideCredentials for Source {
        fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
        where
            Self: 'a,
        {
            future::ProvideCredentials::new(async move {
                self.0.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(5)).await;
                Ok(Credentials::new(
                    "synthetic-key",
                    "synthetic-secret",
                    None,
                    Some(SystemTime::now() + Duration::from_secs(3600)),
                    "fixture",
                ))
            })
        }
    }
    #[tokio::test]
    async fn concurrent_signers_share_one_native_refresh_and_expiry_renews_it()
    -> Result<(), Box<dyn std::error::Error>> {
        let calls = Arc::new(AtomicUsize::new(0));
        let cached = Cached::new(SharedCredentialsProvider::new(Source(calls.clone())));
        for result in futures::future::join_all((0..16).map(|_| cached.provide_credentials())).await
        {
            assert_eq!(result?.access_key_id(), "synthetic-key");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        *cached.value.lock().await = Some(Credentials::new(
            "old",
            "old",
            None,
            Some(SystemTime::UNIX_EPOCH),
            "fixture",
        ));
        assert_eq!(
            cached.provide_credentials().await?.access_key_id(),
            "synthetic-key"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        Ok(())
    }
}

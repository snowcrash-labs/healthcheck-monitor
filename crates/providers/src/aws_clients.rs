//! One native SDK client per explicitly configured AWS region.
use aws_config::SdkConfig;
use monitor_integrations::transport::Error;
pub struct AwsClients {
    pub config: SdkConfig,
    pub sts: aws_sdk_sts::Client,
    services: scc::HashCache<String, std::sync::Arc<dyn std::any::Any + Send + Sync>>,
}
impl AwsClients {
    pub fn new(mut config: SdkConfig) -> Self {
        if let Some(provider) = config.credentials_provider() {
            config = config
                .to_builder()
                .credentials_provider(
                    aws_credential_types::provider::SharedCredentialsProvider::new(
                        crate::aws_credential_cache::Cached::new(provider),
                    ),
                )
                .build();
        }
        Self {
            sts: aws_sdk_sts::Client::new(&config),
            config,
            services: scc::HashCache::with_capacity(0, 1024),
        }
    }
    /// Client identity includes per-check retry and timeout policy, while transport pools stay shared.
    pub async fn service<T: Clone + Send + Sync + 'static>(
        &self,
        name: &str,
        region: &str,
        settings: &monitor_core::config::settings::Settings,
        build: fn(&SdkConfig) -> T,
    ) -> Result<T, Error> {
        let key = format!(
            "{name}/{region}/{}/{}/{}/{}",
            settings.connect_timeout.0,
            settings.attempt_timeout.0,
            settings.operation_timeout.0,
            settings.attempts
        );
        if let Some(entry) = self.services.get_async(&key).await {
            return entry
                .get()
                .downcast_ref::<T>()
                .cloned()
                .ok_or(Error::Malformed);
        }
        let config = self
            .config
            .to_builder()
            .region(aws_config::Region::new(region.to_string()))
            .timeout_config(
                aws_config::timeout::TimeoutConfig::builder()
                    .connect_timeout(settings.connect_timeout.duration())
                    .operation_timeout(settings.operation_timeout.duration())
                    .operation_attempt_timeout(settings.attempt_timeout.duration())
                    .build(),
            )
            .retry_config(
                aws_config::retry::RetryConfig::standard()
                    .with_max_attempts(settings.attempts as u32),
            )
            .build();
        let (_, entry) = self
            .services
            .entry_async(key)
            .await
            .or_put_with(|| std::sync::Arc::new(build(&config)));
        entry
            .get()
            .downcast_ref::<T>()
            .cloned()
            .ok_or(Error::Malformed)
    }
    pub async fn cloudwatch(
        &self,
        region: &str,
        settings: &monitor_core::config::settings::Settings,
    ) -> Result<aws_sdk_cloudwatch::Client, Error> {
        self.service(
            "cloudwatch",
            region,
            settings,
            aws_sdk_cloudwatch::Client::new,
        )
        .await
    }
    pub async fn signals(
        &self,
        region: &str,
        settings: &monitor_core::config::settings::Settings,
    ) -> Result<aws_sdk_applicationsignals::Client, Error> {
        self.service(
            "signals",
            region,
            settings,
            aws_sdk_applicationsignals::Client::new,
        )
        .await
    }
}

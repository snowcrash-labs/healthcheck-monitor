//! One native SDK client per explicitly configured AWS region.
use aws_config::SdkConfig;
use monitor_integrations::transport::Error;
pub struct AwsClients {
    pub config: SdkConfig,
    pub sts: aws_sdk_sts::Client,
    cloudwatch: scc::HashMap<String, aws_sdk_cloudwatch::Client>,
}
impl AwsClients {
    pub fn new(config: SdkConfig) -> Self {
        Self {
            sts: aws_sdk_sts::Client::new(&config),
            config,
            cloudwatch: scc::HashMap::new(),
        }
    }
    pub async fn cloudwatch(&self, region: &str) -> Result<aws_sdk_cloudwatch::Client, Error> {
        if let Some(client) = self
            .cloudwatch
            .read_async(region, |_, client| client.clone())
            .await
        {
            return Ok(client);
        }
        if self.cloudwatch.len() >= 32 {
            return Err(Error::Limit);
        }
        let config = self
            .config
            .to_builder()
            .region(aws_config::Region::new(region.to_string()))
            .build();
        let entry = self
            .cloudwatch
            .entry_async(region.to_string())
            .await
            .or_insert_with(|| aws_sdk_cloudwatch::Client::new(&config));
        Ok(entry.get().clone())
    }
}

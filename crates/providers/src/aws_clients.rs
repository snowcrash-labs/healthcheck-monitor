//! One native SDK client per explicitly configured AWS region.
use aws_config::SdkConfig;
use monitor_integrations::transport::Error;
pub struct AwsClients {
    pub config: SdkConfig,
    pub sts: aws_sdk_sts::Client,
    regions: scc::HashMap<String, RegionClients>,
}
#[derive(Clone)]
struct RegionClients {
    cloudwatch: aws_sdk_cloudwatch::Client,
    signals: aws_sdk_applicationsignals::Client,
}
impl AwsClients {
    pub fn new(config: SdkConfig) -> Self {
        Self {
            sts: aws_sdk_sts::Client::new(&config),
            config,
            regions: scc::HashMap::new(),
        }
    }
    pub async fn cloudwatch(&self, region: &str) -> Result<aws_sdk_cloudwatch::Client, Error> {
        Ok(self.region(region).await?.cloudwatch)
    }
    pub async fn signals(&self, region: &str) -> Result<aws_sdk_applicationsignals::Client, Error> {
        Ok(self.region(region).await?.signals)
    }
    async fn region(&self, region: &str) -> Result<RegionClients, Error> {
        if let Some(client) = self
            .regions
            .read_async(region, |_, client| client.clone())
            .await
        {
            return Ok(client);
        }
        if self.regions.len() >= 32 {
            return Err(Error::Limit);
        }
        let config = self
            .config
            .to_builder()
            .region(aws_config::Region::new(region.to_string()))
            .build();
        let entry = self
            .regions
            .entry_async(region.to_string())
            .await
            .or_insert_with(|| RegionClients {
                cloudwatch: aws_sdk_cloudwatch::Client::new(&config),
                signals: aws_sdk_applicationsignals::Client::new(&config),
            });
        Ok(entry.get().clone())
    }
}

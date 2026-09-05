//! Successful empty inventories suppress unused namespaces; active services require telemetry.
use crate::common::{Endpoint, Source};
use monitor_core::{config::resolve::Job, model::*};
use std::collections::{BTreeMap, BTreeSet};
use tokio_util::sync::CancellationToken;
pub type Namespaces = BTreeMap<String, BTreeSet<String>>;
pub async fn register<S: Source>(
    source: &S,
    job: &Job,
    cancel: &CancellationToken,
) -> (Namespaces, CheckResult) {
    let mut inventory = job.clone();
    inventory.check = Check::Inventory;
    let endpoints = crate::aws::endpoints(&inventory)
        .into_iter()
        .filter(|endpoint| {
            let family = endpoint.id.split('/').next().unwrap_or("");
            !namespaces(family).is_empty()
                && (job.check != Check::Queues || matches!(family, "sqs" | "sns" | "eventbridge"))
        });
    let mut active = Namespaces::new();
    let mut result = crate::router::base(job);
    for endpoint in endpoints {
        let key = crate::common::cache_key(job, &endpoint);
        let fetched = if let Some(cache) = source.cache() {
            cache.load(source, job, endpoint.clone(), cancel, key).await
        } else {
            crate::endpoint_scan::fetch(source, job, endpoint.clone(), cancel).await
        };
        register_endpoint(&mut active, &endpoint, &fetched.result);
        result.operations.extend(fetched.result.operations);
    }
    result.finished_at = chrono::Utc::now();
    (active, result)
}
fn register_endpoint(active: &mut Namespaces, endpoint: &Endpoint, result: &CheckResult) {
    if result
        .operations
        .iter()
        .all(|operation| operation.coverage == Coverage::Complete && operation.records == 0)
    {
        return;
    }
    let Some((_, region, _)) = &endpoint.aws else {
        return;
    };
    let family = endpoint.id.split('/').next().unwrap_or("");
    for namespace in namespaces(family) {
        if family == "load-balancers" && result.complete() {
            let application = result
                .observations
                .iter()
                .any(|observation| observation.resource.contains("loadbalancer/app/"));
            let network = result
                .observations
                .iter()
                .any(|observation| observation.resource.contains("loadbalancer/net/"));
            if (application || network)
                && ((*namespace == "AWS/ApplicationELB" && !application)
                    || (*namespace == "AWS/NetworkELB" && !network))
            {
                continue;
            }
        }
        active
            .entry(region.clone())
            .or_default()
            .insert((*namespace).into());
    }
}
fn namespaces(family: &str) -> &'static [&'static str] {
    match family {
        "ec2" => &["AWS/EC2"],
        "ebs" => &["AWS/EBS"],
        "autoscaling" => &["AWS/AutoScaling"],
        "eks" => &["AWS/EKS"],
        "ecs-clusters" => &["AWS/ECS"],
        "lambda" => &["AWS/Lambda"],
        "load-balancers" => &["AWS/ApplicationELB", "AWS/NetworkELB"],
        "cloudfront" => &["AWS/CloudFront"],
        "rds" | "aurora" => &["AWS/RDS"],
        "elasticache" | "replication-groups" => &["AWS/ElastiCache"],
        "dynamodb" => &["AWS/DynamoDB"],
        "s3" => &["AWS/S3"],
        "sqs" => &["AWS/SQS"],
        "sns" => &["AWS/SNS"],
        "eventbridge" => &["AWS/Events"],
        "codebuild" => &["AWS/CodeBuild"],
        _ => &[],
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use monitor_integrations::transport::Error;
    struct Empty;
    impl Source for Empty {
        async fn request(
            &self,
            _: &Endpoint,
            _: &Job,
            _: &CancellationToken,
        ) -> Result<serde_json::Value, Error> {
            Ok(serde_json::json!({}))
        }
    }
    struct Cached {
        cache: crate::inventory_cache::InventoryCache,
        calls: std::sync::atomic::AtomicUsize,
    }
    impl Source for Cached {
        fn cache(&self) -> Option<&crate::inventory_cache::InventoryCache> {
            Some(&self.cache)
        }
        async fn request(
            &self,
            _: &Endpoint,
            _: &Job,
            _: &CancellationToken,
        ) -> Result<serde_json::Value, Error> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(serde_json::json!({}))
        }
    }
    #[tokio::test]
    async fn metric_registration_and_inventory_share_one_read_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut job=monitor_core::config::types::Config::parse("version=1\n[[targets]]\nname='aws'\nprovider='aws'\nscope='123456789012'\nregions=['us-east-1']")?.resolve(&Default::default())?.jobs.into_iter().find(|job|job.check==Check::Metrics).ok_or("missing job")?;
        let source = Cached {
            cache: crate::inventory_cache::InventoryCache::new(
                128,
                std::sync::Arc::new(monitor_core::budget::Budget::new(1024 * 1024)),
            ),
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let _ = register(&source, &job, &CancellationToken::new()).await;
        let first = source.calls.load(std::sync::atomic::Ordering::SeqCst);
        job.check = Check::Inventory;
        let endpoints = crate::aws::endpoints(&job)
            .into_iter()
            .filter(|endpoint| !namespaces(endpoint.id.split('/').next().unwrap_or("")).is_empty())
            .collect();
        let result =
            crate::common::collect_from(&source, &job, endpoints, &CancellationToken::new()).await;
        assert!(result.complete());
        assert!(first > 0);
        assert_eq!(
            source.calls.load(std::sync::atomic::Ordering::SeqCst),
            first
        );
        Ok(())
    }
    #[tokio::test]
    async fn empty_account_has_complete_inventory_without_required_unused_metrics()
    -> Result<(), Box<dyn std::error::Error>> {
        let job=monitor_core::config::types::Config::parse("version=1\n[[targets]]\nname='aws'\nprovider='aws'\nscope='123456789012'\nregions=['eu-west-1']")?.resolve(&Default::default())?.jobs.into_iter().find(|job|job.check==Check::Metrics).ok_or("missing job")?;
        let (active, result) = register(&Empty, &job, &CancellationToken::new()).await;
        assert!(active.is_empty());
        assert!(result.complete());
        assert!(result.observations.is_empty());
        Ok(())
    }
    #[test]
    fn denied_inventory_does_not_suppress_independent_metric_collection() {
        let mut endpoint =
            Endpoint::get("rds/eu-west-1", "https://rds.eu-west-1.amazonaws.com/", "");
        endpoint.aws = Some((
            "rds".into(),
            "eu-west-1".into(),
            "query:DescribeDBInstances".into(),
        ));
        let result = CheckResult::failure(
            "test".into(),
            Check::Metrics,
            "revision".into(),
            Coverage::Denied,
        );
        let mut active = Namespaces::new();
        register_endpoint(&mut active, &endpoint, &result);
        assert!(
            active
                .get("eu-west-1")
                .is_some_and(|namespaces| namespaces.contains("AWS/RDS"))
        );
    }
}

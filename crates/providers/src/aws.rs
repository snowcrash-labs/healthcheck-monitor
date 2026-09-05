//! AWS account identity, native CloudWatch batches, and signed read-only APIs.
use crate::{
    auth::Auth,
    common::{self, Endpoint},
};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{
    projection::{observation, operation},
    transport::{Error, Http},
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

pub fn endpoints(job: &Job) -> Vec<Endpoint> {
    let mut out = Vec::new();
    for region in &job.target.regions {
        for (name, service, prefix, action, items) in JSON_APIS {
            if job.check == Check::Alerts && *name != "health" {
                continue;
            }
            if job.check == Check::Releases
                && !matches!(*name, "ecr" | "codebuild" | "codepipeline" | "ecs-services")
            {
                continue;
            }
            let mut endpoint = Endpoint::get(
                format!("{name}/{region}"),
                format!("https://{service}.{region}.amazonaws.com/"),
                items,
            );
            endpoint.aws = Some((
                (*service).into(),
                region.clone(),
                format!("{prefix}.{action}"),
            ));
            endpoint.body = Some(match *name {
                "quotas" => json!({"ServiceCode":"ec2","MaxResults":100}),
                "health" => {
                    json!({"filter":{"eventStatusCodes":["open","upcoming"]},"maxResults":100})
                }
                "dynamodb" => json!({"Limit":100}),
                _ => json!({}),
            });
            out.push(endpoint);
        }
        for (name, service, action, version, items) in QUERY_APIS {
            if matches!(job.check, Check::Alerts | Check::Releases | Check::Logs) {
                continue;
            }
            let mut endpoint = Endpoint::get(
                format!("{name}/{region}"),
                format!("https://{service}.{region}.amazonaws.com/"),
                items,
            );
            endpoint.aws = Some(((*service).into(), region.clone(), format!("query:{action}")));
            endpoint.body = Some(json!({"Action":action,"Version":version}));
            out.push(endpoint);
        }
        for (name, service, path, items) in [
            ("eks", "eks", "/clusters", "/clusters"),
            ("lambda", "lambda", "/2015-03-31/functions/", "/Functions"),
            ("s3", "s3", "/", "/Buckets/Bucket"),
            ("backup", "backup", "/backup-vaults/", "/BackupVaultList"),
            ("acm", "acm", "/", "/CertificateSummaryList"),
        ] {
            if name == "acm" {
                continue;
            }
            let mut endpoint = Endpoint::get(
                format!("{name}/{region}"),
                format!("https://{service}.{region}.amazonaws.com{path}"),
                items,
            );
            endpoint.aws = Some((service.into(), region.clone(), "".into()));
            out.push(endpoint);
        }
    }
    if job.check == Check::Discovery {
        let mut endpoint = Endpoint::get(
            "organization-accounts",
            "https://organizations.us-east-1.amazonaws.com/",
            "/Accounts",
        );
        endpoint.body = Some(json!({"MaxResults":20}));
        endpoint.aws = Some((
            "organizations".into(),
            "us-east-1".into(),
            "AWSOrganizationsV20161128.ListAccounts".into(),
        ));
        out.push(endpoint);
    }
    out
}
pub async fn collect(
    http: &Http,
    auth: &Auth,
    job: &Job,
    cancel: &CancellationToken,
) -> CheckResult {
    if job.check == Check::Preflight {
        return identity(auth, job).await;
    }
    if job.check == Check::Metrics || job.check == Check::Queues {
        return crate::metrics::aws(auth, job, cancel).await;
    }
    common::collect(http, auth, job, endpoints(job), cancel).await
}
async fn identity(auth: &Auth, job: &Job) -> CheckResult {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Unauthenticated,
    );
    let Auth::Aws(config) = auth else {
        return result;
    };
    let outcome = aws_sdk_sts::Client::new(config)
        .get_caller_identity()
        .send()
        .await;
    match outcome {
        Ok(identity) if identity.account() == Some(job.target.scope.as_str()) => {
            result.operations = vec![operation("caller-identity", Ok(1), 1, true)];
            result.observations.push(observation(
                job,
                "caller-identity",
                "current",
                Data::Identity {
                    scope: identity
                        .arn()
                        .map(monitor_integrations::projection::identity)
                        .unwrap_or_default(),
                },
            ));
        }
        Ok(_) => {
            result.operations = vec![operation(
                "caller-identity",
                Err(&Error::Forbidden),
                1,
                true,
            )]
        }
        Err(_) => {}
    }
    result
}
const JSON_APIS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "ecs-clusters",
        "ecs",
        "AmazonEC2ContainerServiceV20141113",
        "ListClusters",
        "/clusterArns",
    ),
    (
        "ecr",
        "ecr",
        "AmazonEC2ContainerRegistry_V20150921",
        "DescribeRepositories",
        "/repositories",
    ),
    (
        "dynamodb",
        "dynamodb",
        "DynamoDB_20120810",
        "ListTables",
        "/TableNames",
    ),
    ("sqs", "sqs", "AmazonSQS", "ListQueues", "/QueueUrls"),
    ("eventbridge", "events", "AWSEvents", "ListRules", "/Rules"),
    (
        "codebuild",
        "codebuild",
        "CodeBuild_20161006",
        "ListBuilds",
        "/ids",
    ),
    (
        "codepipeline",
        "codepipeline",
        "CodePipeline_20150709",
        "ListPipelines",
        "/pipelines",
    ),
    ("kms", "kms", "TrentService", "ListKeys", "/Keys"),
    (
        "secrets",
        "secretsmanager",
        "secretsmanager",
        "ListSecrets",
        "/SecretList",
    ),
    (
        "quotas",
        "servicequotas",
        "ServiceQuotasV20190624",
        "ListServiceQuotas",
        "/Quotas",
    ),
    (
        "health",
        "health",
        "AWSHealth_20160804",
        "DescribeEvents",
        "/events",
    ),
    (
        "logs",
        "logs",
        "Logs_20140328",
        "DescribeLogGroups",
        "/logGroups",
    ),
];
const QUERY_APIS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "ec2",
        "ec2",
        "DescribeInstances",
        "2016-11-15",
        "/reservationSet/item",
    ),
    (
        "instance-status",
        "ec2",
        "DescribeInstanceStatus",
        "2016-11-15",
        "/instanceStatusSet/item",
    ),
    (
        "ebs",
        "ec2",
        "DescribeVolumes",
        "2016-11-15",
        "/volumeSet/item",
    ),
    (
        "autoscaling",
        "autoscaling",
        "DescribeAutoScalingGroups",
        "2011-01-01",
        "/DescribeAutoScalingGroupsResult/AutoScalingGroups/member",
    ),
    (
        "load-balancers",
        "elasticloadbalancing",
        "DescribeLoadBalancers",
        "2015-12-01",
        "/DescribeLoadBalancersResult/LoadBalancers/member",
    ),
    (
        "target-groups",
        "elasticloadbalancing",
        "DescribeTargetGroups",
        "2015-12-01",
        "/DescribeTargetGroupsResult/TargetGroups/member",
    ),
    (
        "rds",
        "rds",
        "DescribeDBInstances",
        "2014-10-31",
        "/DescribeDBInstancesResult/DBInstances/DBInstance",
    ),
    (
        "aurora",
        "rds",
        "DescribeDBClusters",
        "2014-10-31",
        "/DescribeDBClustersResult/DBClusters/DBCluster",
    ),
    (
        "elasticache",
        "elasticache",
        "DescribeCacheClusters",
        "2015-02-02",
        "/DescribeCacheClustersResult/CacheClusters/CacheCluster",
    ),
    (
        "replication-groups",
        "elasticache",
        "DescribeReplicationGroups",
        "2015-02-02",
        "/DescribeReplicationGroupsResult/ReplicationGroups/ReplicationGroup",
    ),
    (
        "sns",
        "sns",
        "ListTopics",
        "2010-03-31",
        "/ListTopicsResult/Topics/member",
    ),
];

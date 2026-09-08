//! Follow-up reads derived from selected resource identities.
use crate::common::Endpoint;
use monitor_core::config::resolve::Job;
use serde_json::json;
pub fn followups(
    job: &Job,
    parent: &Endpoint,
    family: &str,
    name: &str,
    row: &serde_json::Value,
) -> Vec<Endpoint> {
    let Some((_, region, _)) = &parent.aws else {
        return vec![];
    };
    // Query quota details for monitored service families, not every AWS product in the catalog.
    if family == "quota-services"
        && ![
            "ec2",
            "autoscaling",
            "eks",
            "ecs",
            "lambda",
            "elasticloadbalancing",
            "rds",
            "elasticache",
            "dynamodb",
            "s3",
            "sqs",
            "sns",
            "events",
            "ecr",
            "codebuild",
            "codepipeline",
            "kms",
            "secretsmanager",
            "backup",
            "route53",
            "cloudfront",
            "acm",
            "monitoring",
        ]
        .contains(&name)
    {
        return vec![];
    }
    let description = match family {
        "quota-services" => Some((
            "quotas",
            "servicequotas",
            "ServiceQuotasV20190624.ListServiceQuotas",
            json!({"ServiceCode":name,"MaxResults":job.settings.page_size.min(100)}),
            "/Quotas",
        )),
        "acm" => Some((
            "acm-detail",
            "acm",
            "CertificateManager.DescribeCertificate",
            json!({"CertificateArn":name}),
            "/Certificate",
        )),
        "secrets" => Some((
            "secret-versions",
            "secretsmanager",
            "secretsmanager.ListSecretVersionIds",
            json!({"SecretId":name,"MaxResults":100}),
            "/Versions",
        )),
        "dynamodb" => Some((
            "dynamodb-detail",
            "dynamodb",
            "DynamoDB_20120810.DescribeTable",
            json!({"TableName":name}),
            "/Table",
        )),
        "dynamodb-detail" => Some((
            "dynamodb-backups",
            "dynamodb",
            "DynamoDB_20120810.DescribeContinuousBackups",
            json!({"TableName":name}),
            "/ContinuousBackupsDescription",
        )),
        "ecs-clusters" => Some((
            "ecs-services",
            "ecs",
            "AmazonEC2ContainerServiceV20141113.ListServices",
            json!({"cluster":name}),
            "/serviceArns",
        )),
        "ecs-services" => Some((
            "ecs-services-detail",
            "ecs",
            "AmazonEC2ContainerServiceV20141113.DescribeServices",
            json!({"cluster":parent.body.as_ref().and_then(|b|b.get("cluster")),"services":[name]}),
            "/services",
        )),
        "sqs" => Some((
            "sqs-attributes",
            "sqs",
            "AmazonSQS.GetQueueAttributes",
            json!({"QueueUrl":name,"AttributeNames":["ApproximateNumberOfMessages","ApproximateNumberOfMessagesNotVisible","ApproximateNumberOfMessagesDelayed","VisibilityTimeout","MessageRetentionPeriod","RedrivePolicy","KmsMasterKeyId","SqsManagedSseEnabled"]}),
            "",
        )),
        "ecr" => Some((
            "registry-images",
            "ecr",
            "AmazonEC2ContainerRegistry_V20150921.DescribeImages",
            json!({"repositoryName":name,"maxResults":100}),
            "/imageDetails",
        )),
        "codebuild" => Some((
            "build-details",
            "codebuild",
            "CodeBuild_20161006.BatchGetBuilds",
            json!({"ids":[name]}),
            "/builds",
        )),
        "codepipeline" => Some((
            "pipeline-state",
            "codepipeline",
            "CodePipeline_20150709.GetPipelineState",
            json!({"name":name}),
            "",
        )),
        "kms" => Some((
            "kms-detail",
            "kms",
            "TrentService.DescribeKey",
            json!({"KeyId":name}),
            "/KeyMetadata",
        )),
        _ => None,
    };
    if let Some((id, service, target, body, items)) = description {
        let mut endpoint = Endpoint::get(
            format!("{id}/{region}/{name}"),
            format!("https://{service}.{region}.amazonaws.com/"),
            items,
        );
        endpoint.aws = Some((service.into(), region.clone(), target.into()));
        endpoint.body = Some(body);
        if family == "ecs-clusters" {
            let mut tasks = endpoint.clone();
            tasks.id = format!("ecs-tasks/{region}/{name}");
            tasks.items = "/taskArns".into();
            tasks.aws = Some((
                "ecs".into(),
                region.clone(),
                "AmazonEC2ContainerServiceV20141113.ListTasks".into(),
            ));
            tasks.body = Some(json!({"cluster":name,"desiredStatus":"RUNNING","maxResults":100}));
            return vec![endpoint, tasks];
        }
        return vec![endpoint];
    }
    if family == "eks" {
        let mut endpoint = Endpoint::get(
            format!("eks-detail/{region}/{name}"),
            format!("https://eks.{region}.amazonaws.com/clusters/{name}"),
            "/cluster",
        );
        endpoint.aws = Some(("eks".into(), region.clone(), "".into()));
        return vec![endpoint];
    }
    if family == "s3" {
        return [
            "encryption",
            "versioning",
            "replication",
            "publicAccessBlock",
        ]
        .into_iter()
        .map(|config| {
            let mut endpoint = Endpoint::get(
                format!("s3-{config}/{region}/{name}"),
                format!("https://s3.{region}.amazonaws.com/{name}?{config}"),
                "",
            );
            endpoint.aws = Some(("s3".into(), region.clone(), "".into()));
            endpoint
        })
        .collect();
    }
    let rest = match family {
        "lambda"
            if monitor_integrations::projection::text(row, &["/PackageType"]) == Some("Image") =>
        {
            Some((
                "lambda-image",
                "lambda",
                format!("/2015-03-31/functions/{name}"),
                "",
            ))
        }
        "lambda" => Some((
            "lambda-detail",
            "lambda",
            format!("/2015-03-31/functions/{name}/configuration"),
            "",
        )),
        "backup" => Some((
            "recovery-points",
            "backup",
            format!(
                "/backup-vaults/{name}/recovery-points?maxResults={}",
                job.settings.page_size
            ),
            "/RecoveryPoints",
        )),
        "route53" => Some((
            "dns-records",
            "route53",
            format!("/2013-04-01/{}/rrset", name.trim_start_matches('/')),
            "/ResourceRecordSets/ResourceRecordSet",
        )),
        _ => None,
    };
    if let Some((id, service, path, items)) = rest {
        let host = if service == "route53" {
            "route53.amazonaws.com".into()
        } else {
            format!("{service}.{region}.amazonaws.com")
        };
        let mut endpoint = Endpoint::get(
            format!("{id}/{region}/{name}"),
            format!("https://{host}{path}"),
            items,
        );
        endpoint.aws = Some((service.into(), region.clone(), "".into()));
        return vec![endpoint];
    }
    if family == "target-groups" {
        let mut endpoint = Endpoint::get(
            format!("target-health/{name}"),
            format!("https://elasticloadbalancing.{region}.amazonaws.com/"),
            "/DescribeTargetHealthResult/TargetHealthDescriptions/member",
        );
        endpoint.aws = Some((
            "elasticloadbalancing".into(),
            region.clone(),
            "query:DescribeTargetHealth".into(),
        ));
        endpoint.body = Some(
            json!({"Action":"DescribeTargetHealth","Version":"2015-12-01","TargetGroupArn":name}),
        );
        return vec![endpoint];
    }
    vec![]
}

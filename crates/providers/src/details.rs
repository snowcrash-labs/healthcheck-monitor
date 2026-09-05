//! Bounded follow-up reads derived only from discovered resource identifiers.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::Provider};
use monitor_integrations::projection::text;
use serde_json::{Value, json};
pub fn followups(job: &Job, parent: &Endpoint, row: &Value) -> Vec<Endpoint> {
    let family = parent.id.split('/').next().unwrap_or("");
    let name = text(
        row,
        &[
            "/name",
            "/id",
            "/Name",
            "/KeyId",
            "/TableName",
            "/repositoryName",
            "/TargetGroupArn",
            "/Id",
            "/CertificateArn",
            "/BackupVaultName",
        ],
    )
    .or_else(|| row.as_str());
    let Some(name) = name else { return vec![] };
    if !monitor_core::config::validate::identifier(name) {
        return vec![];
    }
    match job.target.provider {
        Provider::Gcp => gcp(job, parent, family, name, row),
        Provider::Aws => aws(job, parent, family, name),
        Provider::Azure => azure(job, parent, family, name),
        _ => vec![],
    }
}
fn gcp(job: &Job, parent: &Endpoint, family: &str, name: &str, row: &Value) -> Vec<Endpoint> {
    let p = &job.target.scope;
    let mut out = Vec::new();
    let paths: Vec<(String, String, &str)> = match family {
        "sql" => vec![
            (
                format!("sql-backups/{name}"),
                format!(
                    "sqladmin.googleapis.com/sql/v1beta4/projects/{p}/instances/{name}/backupRuns?maxResults=5"
                ),
                "/items",
            ),
            (
                format!("sql-operations/{name}"),
                format!(
                    "sqladmin.googleapis.com/sql/v1beta4/projects/{p}/operations?instance={name}&maxResults=20"
                ),
                "/items",
            ),
        ],
        "dns-zones" => vec![(
            format!("dns-records/{name}"),
            format!("dns.googleapis.com/dns/v1/projects/{p}/managedZones/{name}/rrsets"),
            "/rrsets",
        )],
        "kms-keyrings" => vec![(
            format!("kms-keys/{name}"),
            format!("cloudkms.googleapis.com/v1/{name}/cryptoKeys"),
            "/cryptoKeys",
        )],
        "kms-keys" => vec![(
            format!("kms-versions/{name}"),
            format!("cloudkms.googleapis.com/v1/{name}/cryptoKeyVersions"),
            "/cryptoKeyVersions",
        )],
        "secrets" => vec![(
            format!("secret-versions/{name}"),
            format!("secretmanager.googleapis.com/v1/{name}/versions"),
            "/versions",
        )],
        "buckets" => vec![(
            format!("bucket-config/{name}"),
            format!("storage.googleapis.com/storage/v1/b/{name}"),
            "",
        )],
        "artifact-repositories" => vec![(
            format!("registry-images/{name}"),
            format!("artifactregistry.googleapis.com/v1/{name}/dockerImages"),
            "/dockerImages",
        )],
        "slo-services" => vec![(
            format!("slo-objectives/{name}"),
            format!("monitoring.googleapis.com/v3/{name}/serviceLevelObjectives"),
            "/serviceLevelObjectives",
        )],
        _ => vec![],
    };
    for (id, url, items) in paths {
        out.push(Endpoint::get(id, format!("https://{url}"), items));
    }
    if family == "backend-services"
        && let Some(backends) = row.get("backends").and_then(Value::as_array)
    {
        for (i, backend) in backends.iter().take(100).enumerate() {
            if let Some(group) = text(backend, &["/group"]) {
                let mut endpoint = Endpoint::get(
                    format!("backend-health/{name}/{i}"),
                    format!("{}/{name}/getHealth", parent.url),
                    "/healthStatus",
                );
                endpoint.body = Some(json!({"group":group}));
                out.push(endpoint);
            }
        }
    }
    out
}
fn aws(job: &Job, parent: &Endpoint, family: &str, name: &str) -> Vec<Endpoint> {
    let Some((_, region, _)) = &parent.aws else {
        return vec![];
    };
    let description = match family {
        "dynamodb" => Some((
            "dynamodb-detail",
            "dynamodb",
            "DynamoDB_20120810.DescribeTable",
            json!({"TableName":name}),
            "/Table",
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
    let _ = job;
    vec![]
}
fn azure(job: &Job, _parent: &Endpoint, family: &str, name: &str) -> Vec<Endpoint> {
    if !name.starts_with(&format!("/subscriptions/{}/", job.target.scope)) {
        return vec![];
    }
    let (id, path, version) = match family {
        "service-bus" => ("service-bus-queues", "queues", "2024-01-01"),
        "event-hubs" => ("event-hub-details", "eventhubs", "2024-01-01"),
        "sql" => ("sql-databases", "databases", "2023-08-01"),
        "backup-vaults" => (
            "backup-protected-items",
            "backupProtectedItems",
            "2024-04-01",
        ),
        "vms" => ("vm-instance-view", "instanceView", "2024-11-01"),
        _ => return vec![],
    };
    vec![Endpoint::get(
        format!("{id}/{name}"),
        format!("https://management.azure.com{name}/{path}?api-version={version}"),
        if family == "vms" { "" } else { "/value" },
    )]
}

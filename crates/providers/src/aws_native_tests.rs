//! Contract tests execute native SDK serialization through a synthetic read-policy transport.
use super::{auth::Auth, aws_clients::AwsClients};
use aws_smithy_runtime_api::client::{
    http::{HttpConnector, HttpConnectorFuture, SharedHttpConnector, http_client_fn},
    orchestrator::{HttpRequest, HttpResponse},
    result::ConnectorError,
};
use aws_smithy_types::body::SdkBody;
use monitor_core::{
    config::{resolve::Job, types::Config},
    model::Check,
};
use serde_json::json;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Default)]
struct Recorder {
    requests: Arc<Mutex<Vec<(String, bool)>>>,
}
impl HttpConnector for Recorder {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        let mut builder = reqwest::Client::new()
            .request(
                reqwest::Method::from_bytes(request.method().as_bytes())
                    .unwrap_or(reqwest::Method::POST),
                request.uri(),
            )
            .body(request.body().bytes().unwrap_or(&[]).to_vec());
        for (key, value) in request.headers().iter() {
            builder = builder.header(key, value);
        }
        let allowed = builder
            .build()
            .is_ok_and(|request| monitor_integrations::transport::allowed(&request));
        if let Ok(mut requests) = self.requests.lock() {
            requests.push((request.uri().to_string(), allowed));
        }
        HttpConnectorFuture::new(async {
            let status = aws_smithy_runtime_api::http::StatusCode::try_from(400).map_err(|_| {
                ConnectorError::other(Box::new(std::io::Error::other("fixture")), None)
            })?;
            Ok(HttpResponse::new(
                status,
                SdkBody::from("{\"__type\":\"AccessDeniedException\"}"),
            ))
        })
    }
}
fn auth(recorder: Recorder) -> Auth {
    let connector = SharedHttpConnector::new(recorder);
    let config = aws_config::SdkConfig::builder()
        .behavior_version(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new("us-east-1"))
        .credentials_provider(
            aws_credential_types::provider::SharedCredentialsProvider::new(
                aws_credential_types::Credentials::new(
                    "synthetic-key",
                    "synthetic-secret",
                    None,
                    None,
                    "fixture",
                ),
            ),
        )
        .disable_request_compression(true)
        .retry_config(aws_config::retry::RetryConfig::standard().with_max_attempts(1))
        .http_client(http_client_fn(move |_, _| connector.clone()))
        .build();
    Auth::Aws(Box::new(AwsClients::new(config)))
}
fn job() -> Result<Job, Box<dyn std::error::Error>> {
    let mut job = Config::parse("version=1\n[[targets]]\nname='fixture'\nprovider='aws'\nscope='123456789012'\nregions=['us-east-1']")?.resolve(&Default::default())?.jobs.into_iter().find(|job| job.check == Check::Inventory).ok_or("missing job")?;
    job.settings.attempts = 1;
    Ok(job)
}
#[test]
fn native_dispatcher_frame_stays_small_as_service_sdks_grow()
-> Result<(), Box<dyn std::error::Error>> {
    let auth = Auth::None;
    let job = job()?;
    let endpoint =
        crate::common::Endpoint::get("fixture", "https://ec2.us-east-1.amazonaws.com", "");
    let request = crate::aws_native::request(&auth, &endpoint, &job);
    assert!(std::mem::size_of_val(&request) < 64 * 1024);
    Ok(())
}
#[test]
fn quota_catalogs_cannot_preempt_resource_inventory() -> Result<(), Box<dyn std::error::Error>> {
    let mut job = job()?;
    job.target.regions = vec!["us-east-1".into(), "us-west-2".into()];
    let endpoints = crate::aws::endpoints(&job);
    let quota = endpoints
        .iter()
        .position(|endpoint| endpoint.id.starts_with("quota-services/"))
        .ok_or("quota endpoint")?;
    assert!(
        endpoints[quota..]
            .iter()
            .all(|endpoint| endpoint.id.starts_with("quota-services/"))
    );
    let parent = &endpoints[quota];
    assert!(
        crate::aws_details::followups(
            &job,
            parent,
            "quota-services",
            "unmonitored-product",
            &json!({})
        )
        .is_empty()
    );
    assert_eq!(
        crate::aws_details::followups(&job, parent, "quota-services", "ec2", &json!({})).len(),
        1
    );
    assert!(
        crate::resource_projection::project(
            &job,
            parent,
            &json!({"ServiceCode":"ec2","ServiceName":"EC2"})
        )
        .is_empty()
    );
    Ok(())
}
#[tokio::test]
async fn every_aws_inventory_family_uses_native_serialization_and_allowed_requests()
-> Result<(), Box<dyn std::error::Error>> {
    let recorder = Recorder::default();
    let auth = auth(recorder.clone());
    let mut job = job()?;
    let mut seen = BTreeSet::new();
    for check in [Check::Inventory, Check::Discovery, Check::Releases] {
        job.check = check;
        for endpoint in crate::aws::endpoints(&job) {
            let key = (endpoint.id.clone(), endpoint.url.clone());
            if !seen.insert(key) {
                continue;
            }
            assert!(
                crate::aws_native::request(&auth, &endpoint, &job)
                    .await
                    .is_some(),
                "{}",
                endpoint.id
            );
        }
    }
    let requests = recorder.requests.lock().map_err(|_| "poisoned")?;
    assert_eq!(requests.len(), seen.len());
    let denied: Vec<_> = requests.iter().filter(|(_, allowed)| !allowed).collect();
    assert!(denied.is_empty(), "{denied:?}");
    Ok(())
}
#[tokio::test]
async fn native_followup_requests_preserve_read_only_policy()
-> Result<(), Box<dyn std::error::Error>> {
    let recorder = Recorder::default();
    let auth = auth(recorder.clone());
    let job = job()?;
    for (service, action, body) in [
        (
            "sqs",
            "AmazonSQS.GetQueueAttributes",
            json!({"QueueUrl":"https://sqs.us-east-1.amazonaws.com/123456789012/fixture","AttributeNames":["ApproximateNumberOfMessages"]}),
        ),
        (
            "ecs",
            "AmazonEC2ContainerServiceV20141113.DescribeTasks",
            json!({"cluster":"fixture","tasks":["fixture"]}),
        ),
        (
            "ecs",
            "AmazonEC2ContainerServiceV20141113.DescribeServices",
            json!({"cluster":"fixture","services":["fixture"]}),
        ),
        (
            "kms",
            "TrentService.DescribeKey",
            json!({"KeyId":"fixture"}),
        ),
        (
            "secretsmanager",
            "secretsmanager.ListSecretVersionIds",
            json!({"SecretId":"fixture"}),
        ),
        (
            "ecr",
            "AmazonEC2ContainerRegistry_V20150921.DescribeImages",
            json!({"repositoryName":"fixture"}),
        ),
        (
            "codebuild",
            "CodeBuild_20161006.BatchGetBuilds",
            json!({"ids":["fixture"]}),
        ),
        (
            "codepipeline",
            "CodePipeline_20150709.ListPipelineExecutions",
            json!({"pipelineName":"fixture"}),
        ),
        (
            "elasticloadbalancing",
            "query:DescribeTargetHealth",
            json!({"TargetGroupArn":"fixture"}),
        ),
        (
            "dynamodb",
            "DynamoDB_20120810.DescribeContinuousBackups",
            json!({"TableName":"fixture"}),
        ),
        (
            "logs",
            "Logs_20140328.FilterLogEvents",
            json!({"logGroupName":"fixture","filterPattern":"?ERROR","startTime":1800000000000i64,"endTime":1800000001000i64,"limit":10}),
        ),
    ] {
        let mut endpoint = crate::common::Endpoint::get(
            "fixture",
            format!("https://{service}.us-east-1.amazonaws.com/"),
            "",
        );
        endpoint.aws = Some((service.into(), "us-east-1".into(), action.into()));
        endpoint.body = Some(body);
        assert!(
            crate::aws_native::request(&auth, &endpoint, &job)
                .await
                .is_some()
        );
    }
    let requests = recorder.requests.lock().map_err(|_| "poisoned")?;
    assert_eq!(requests.len(), 11);
    assert!(requests.iter().all(|(_, allowed)| *allowed), "{requests:?}");
    Ok(())
}

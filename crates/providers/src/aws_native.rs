//! Native SDK requests retain only fields consumed by the common monitoring projections.
use crate::{auth::Auth, common::Endpoint};
use monitor_core::config::resolve::Job;
use monitor_integrations::transport::Error;
use serde_json::{Value, json};

pub async fn request(auth: &Auth, endpoint: &Endpoint, job: &Job) -> Option<Result<Value, Error>> {
    let Auth::Aws(clients) = auth else {
        return None;
    };
    let (service, region, target) = endpoint.aws.as_ref()?;
    let action = target.rsplit(['.', ':']).next().unwrap_or("");
    Some(match service.as_str() {
        "sqs" | "sns" | "events" => {
            crate::aws_native_queues::request(clients, endpoint, job, region, action).await
        }
        "ecs" => crate::aws_native_ecs::request(clients, endpoint, job, region, action).await,
        "elasticloadbalancing" | "route53" | "cloudfront" => {
            crate::aws_native_edge::request(clients, endpoint, job, region, service, action).await
        }
        "s3" | "backup" => {
            crate::aws_native_storage::request(clients, endpoint, job, region, service).await
        }
        "health" | "organizations" | "servicequotas" => {
            crate::aws_native_governance::request(clients, endpoint, job, region, service, action)
                .await
        }
        "logs" => crate::aws_native_logs::request(clients, endpoint, job, region, action).await,
        "lambda" => crate::aws_native_lambda::request(clients, endpoint, job, region).await,
        "ec2" | "autoscaling" | "eks" => {
            crate::aws_native_compute::request(clients, endpoint, job, region, service, action)
                .await
        }
        "rds" | "elasticache" | "dynamodb" => {
            crate::aws_native_databases::request(clients, endpoint, job, region, service, action)
                .await
        }
        "kms" | "acm" | "secretsmanager" => {
            crate::aws_native_security::request(clients, endpoint, job, region, service, action)
                .await
        }
        "ecr" | "codebuild" | "codepipeline" => {
            crate::aws_native_release::request(clients, endpoint, job, region, service, action)
                .await
        }
        _ => Err(Error::Forbidden),
    })
}
pub fn arg(endpoint: &Endpoint, name: &str) -> Option<String> {
    endpoint
        .body
        .as_ref()
        .and_then(|body| body.get(name))
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| {
            url::Url::parse(&endpoint.url)
                .ok()?
                .query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
        })
}
pub fn required(endpoint: &Endpoint, name: &str) -> Result<String, Error> {
    arg(endpoint, name).ok_or(Error::Malformed)
}
pub fn cursor(endpoint: &Endpoint) -> Option<String> {
    [
        "nextToken",
        "NextToken",
        "Marker",
        "marker",
        "nextMarker",
        "ExclusiveStartTableName",
        "continuation-token",
    ]
    .into_iter()
    .find_map(|name| arg(endpoint, name))
}
pub fn strings(endpoint: &Endpoint, name: &str) -> Result<Vec<String>, Error> {
    endpoint
        .body
        .as_ref()
        .and_then(|body| body.get(name))
        .and_then(Value::as_array)
        .ok_or(Error::Malformed)?
        .iter()
        .map(|value| value.as_str().map(String::from).ok_or(Error::Malformed))
        .collect()
}
pub fn date(value: Option<&aws_smithy_types::DateTime>) -> Option<f64> {
    value.map(|time| time.as_secs_f64())
}
/// Reconstruct the existing list envelope without retaining an unrestricted SDK object.
pub fn page(endpoint: &Endpoint, values: Vec<Value>, token: Option<&str>) -> Value {
    let mut result = Value::Array(values);
    for key in endpoint.items.trim_start_matches('/').split('/').rev() {
        result = json!({key: result});
    }
    if let Some(token) = token {
        result["nextToken"] = json!(token);
    }
    result
}

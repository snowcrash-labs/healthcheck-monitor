//! Cloud metadata projections retain exact native identities instead of display identifiers.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::Observation};
use monitor_integrations::{
    projection::text,
    resource_context::{base, identifier},
};
use serde_json::Value;

pub fn enrich(job: &Job, endpoint: &Endpoint, value: &Value, observations: &mut [Observation]) {
    let native = (job.target.provider == monitor_core::model::Provider::Gcp)
        .then(|| text(value, &["/selfLink", "/name"]))
        .flatten()
        .or_else(|| {
            text(
                value,
                &[
                    "/id",
                    "/arn",
                    "/Arn",
                    "/ResourceArn",
                    "/name",
                    "/Name",
                    "/InstanceId",
                    "/DBInstanceArn",
                    "/DBClusterArn",
                    "/FunctionArn",
                    "/LoadBalancerArn",
                    "/TableArn",
                    "/QueueArn",
                    "/QueueUrl",
                    "/DBInstanceIdentifier",
                    "/CacheClusterId",
                    "/repositoryArn",
                    "/logGroupName",
                ],
            )
        });
    for observation in observations {
        let Some(mut context) = native.and_then(|native| base(job, &endpoint.id, native)) else {
            continue;
        };
        context.region = text(value, &["/location", "/region"])
            .and_then(|s| s.rsplit('/').next())
            .and_then(identifier)
            .or_else(|| endpoint.aws.as_ref().map(|(_, region, _)| region.clone()))
            .or_else(|| {
                if job.target.provider != monitor_core::model::Provider::Gcp {
                    return None;
                }
                let parts: Vec<_> = endpoint.url.split('/').collect();
                parts
                    .windows(2)
                    .find(|pair| matches!(pair[0], "regions" | "locations") && pair[1] != "-")
                    .and_then(|pair| identifier(pair[1]))
            })
            .or(context.region);
        context.zone = text(
            value,
            &["/zone", "/AvailabilityZone", "/Placement/AvailabilityZone"],
        )
        .and_then(|s| s.rsplit('/').next())
        .and_then(identifier);
        context.name = text(
            value,
            &["/name", "/Name", "/DBInstanceIdentifier", "/FunctionName"],
        )
        .and_then(|name| name.rsplit('/').next())
        .and_then(identifier);
        observation.context = Some(context);
    }
}

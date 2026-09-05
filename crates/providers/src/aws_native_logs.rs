//! Native CloudWatch log requests preserve bounded windows; messages go directly to redaction.
use crate::{
    aws_clients::AwsClients,
    aws_errors::sdk,
    aws_native::{cursor, page, required},
    common::Endpoint,
};
use monitor_core::config::resolve::Job;
use monitor_integrations::transport::Error;
use serde_json::{Value, json};
pub async fn request(
    clients: &AwsClients,
    endpoint: &Endpoint,
    job: &Job,
    region: &str,
    action: &str,
) -> Result<Value, Error> {
    let client = clients
        .service(
            "logs",
            region,
            &job.settings,
            aws_sdk_cloudwatchlogs::Client::new,
        )
        .await?;
    Ok(match action {
        "DescribeLogGroups" => {
            let output = client
                .describe_log_groups()
                .limit(job.settings.page_size.min(50) as i32)
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.log_groups().iter().map(|row| json!({"logGroupName":row.log_group_name(),"arn":row.arn(),"retentionInDays":row.retention_in_days()})).collect(), output.next_token())
        }
        "FilterLogEvents" => {
            let body = endpoint.body.as_ref().ok_or(Error::Malformed)?;
            let integer = |key: &str| {
                body.get(key)
                    .and_then(Value::as_i64)
                    .ok_or(Error::Malformed)
            };
            let output = client
                .filter_log_events()
                .log_group_name(required(endpoint, "logGroupName")?)
                .filter_pattern(required(endpoint, "filterPattern")?)
                .start_time(integer("startTime")?)
                .end_time(integer("endTime")?)
                .limit(integer("limit")?.clamp(1, 10000) as i32)
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.events().iter().map(|row| json!({"eventId":row.event_id(),"timestamp":row.timestamp(),"message":row.message()})).collect(), output.next_token())
        }
        _ => return Err(Error::Forbidden),
    })
}

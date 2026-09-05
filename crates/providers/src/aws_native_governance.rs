//! Native organization, quota, and provider-health reads retain only operational metadata.
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
    service: &str,
    action: &str,
) -> Result<Value, Error> {
    if service == "health" {
        let client = clients
            .service(service, region, &job.settings, aws_sdk_health::Client::new)
            .await?;
        let filter = aws_sdk_health::types::EventFilter::builder()
            .event_status_codes(aws_sdk_health::types::EventStatusCode::Open)
            .event_status_codes(aws_sdk_health::types::EventStatusCode::Upcoming)
            .build();
        let output = client
            .describe_events()
            .filter(filter)
            .max_results(job.settings.page_size.clamp(10, 100) as i32)
            .set_next_token(cursor(endpoint))
            .send()
            .await
            .map_err(sdk)?;
        return Ok(page(endpoint, output.events().iter().map(|row| json!({"arn":row.arn(),"service":row.service(),"region":row.region(),"statusCode":row.status_code().map(|state| state.as_str())})).collect(), output.next_token()));
    }
    if service == "organizations" {
        let client = clients
            .service(
                service,
                region,
                &job.settings,
                aws_sdk_organizations::Client::new,
            )
            .await?;
        return Ok(match action {
            "ListAccounts" => {
                let output = client
                    .list_accounts()
                    .max_results(job.settings.page_size.min(20) as i32)
                    .set_next_token(cursor(endpoint))
                    .send()
                    .await
                    .map_err(sdk)?;
                page(endpoint, output.accounts().iter().map(|row| json!({"Id":row.id(),"Arn":row.arn(),"Name":row.name(),"State":row.state().map(|state| state.as_str())})).collect(), output.next_token())
            }
            "DescribeOrganization" => {
                let output = client.describe_organization().send().await.map_err(sdk)?;
                let row = output.organization().ok_or(Error::Missing)?;
                json!({"Organization":{"Id":row.id(),"Arn":row.arn()}})
            }
            _ => return Err(Error::Forbidden),
        });
    }
    let client = clients
        .service(
            "servicequotas",
            region,
            &job.settings,
            aws_sdk_servicequotas::Client::new,
        )
        .await?;
    Ok(match action {
        "ListServices" => {
            let output = client
                .list_services()
                .max_results(job.settings.page_size.min(100) as i32)
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.services().iter().map(|row| json!({"ServiceCode":row.service_code(),"ServiceName":row.service_name()})).collect(), output.next_token())
        }
        "ListServiceQuotas" => {
            let output = client
                .list_service_quotas()
                .service_code(required(endpoint, "ServiceCode")?)
                .max_results(job.settings.page_size.min(100) as i32)
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            let rows = output.quotas().iter().map(|row| {
                let usage = row.usage_metric().map(|usage| json!({"MetricNamespace":usage.metric_namespace(),"MetricName":usage.metric_name(),"MetricDimensions":usage.metric_dimensions()}));
                json!({"QuotaCode":row.quota_code(),"Value":row.value(),"UsageMetric":usage})
            }).collect();
            page(endpoint, rows, output.next_token())
        }
        _ => return Err(Error::Forbidden),
    })
}

//! Native load-balancer and DNS metadata preserve global scope and composite cursors.
use crate::{
    aws_clients::AwsClients,
    aws_errors::sdk,
    aws_native::{arg, cursor, page, required},
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
    if service == "route53" {
        return dns(clients, endpoint, job, region).await;
    }
    if service == "cloudfront" {
        let client = clients
            .service(
                service,
                region,
                &job.settings,
                aws_sdk_cloudfront::Client::new,
            )
            .await?;
        let output = client
            .list_distributions()
            .max_items(job.settings.page_size as i32)
            .set_marker(cursor(endpoint))
            .send()
            .await
            .map_err(sdk)?;
        let list = output.distribution_list().ok_or(Error::Missing)?;
        let mut result = page(endpoint, list.items().iter().map(|row| json!({"Id":row.id(),"DomainName":row.domain_name(),"Status":row.status(),"Enabled":row.enabled()})).collect(), list.next_marker());
        result["IsTruncated"] = json!(list.is_truncated());
        return Ok(result);
    }
    let client = clients
        .service(
            "elbv2",
            region,
            &job.settings,
            aws_sdk_elasticloadbalancingv2::Client::new,
        )
        .await?;
    Ok(match action {
        "DescribeLoadBalancers" => {
            let output = client
                .describe_load_balancers()
                .page_size(job.settings.page_size.min(400) as i32)
                .set_marker(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.load_balancers().iter().map(|row| json!({"LoadBalancerArn":row.load_balancer_arn(),"DNSName":row.dns_name(),"Scheme":row.scheme().map(|scheme| scheme.as_str()),"State":{"Code":row.state().and_then(|state| state.code()).map(|state| state.as_str())}})).collect(), output.next_marker())
        }
        "DescribeTargetGroups" => {
            let output = client
                .describe_target_groups()
                .page_size(job.settings.page_size.min(400) as i32)
                .set_marker(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.target_groups().iter().map(|row| json!({"TargetGroupArn":row.target_group_arn(),"TargetGroupName":row.target_group_name()})).collect(), output.next_marker())
        }
        "DescribeTargetHealth" => {
            let output = client
                .describe_target_health()
                .target_group_arn(required(endpoint, "TargetGroupArn")?)
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.target_health_descriptions().iter().map(|row| json!({"Target":{"Id":row.target().map(|target| target.id()),"Port":row.target().and_then(|target| target.port())},"TargetHealth":{"State":row.target_health().and_then(|state| state.state()).map(|state| state.as_str())}})).collect(), None)
        }
        _ => return Err(Error::Forbidden),
    })
}
async fn dns(
    clients: &AwsClients,
    endpoint: &Endpoint,
    job: &Job,
    region: &str,
) -> Result<Value, Error> {
    let client = clients
        .service(
            "route53",
            region,
            &job.settings,
            aws_sdk_route53::Client::new,
        )
        .await?;
    let url = url::Url::parse(&endpoint.url).map_err(|_| Error::Malformed)?;
    if !url.path().ends_with("/rrset") {
        let output = client
            .list_hosted_zones()
            .max_items(job.settings.page_size as i32)
            .set_marker(cursor(endpoint))
            .send()
            .await
            .map_err(sdk)?;
        let mut result = page(
            endpoint,
            output
                .hosted_zones()
                .iter()
                .map(|row| json!({"Id":row.id(),"Name":row.name()}))
                .collect(),
            output.next_marker(),
        );
        result["IsTruncated"] = json!(output.is_truncated());
        return Ok(result);
    }
    let zone = url
        .path()
        .split("/hostedzone/")
        .nth(1)
        .and_then(|path| path.strip_suffix("/rrset"))
        .ok_or(Error::Malformed)?;
    let output = client
        .list_resource_record_sets()
        .hosted_zone_id(zone)
        .max_items(job.settings.page_size as i32)
        .set_start_record_name(arg(endpoint, "name"))
        .set_start_record_type(
            arg(endpoint, "type").map(|kind| aws_sdk_route53::types::RrType::from(kind.as_str())),
        )
        .set_start_record_identifier(arg(endpoint, "identifier"))
        .send()
        .await
        .map_err(sdk)?;
    // Record content can include verification secrets; inventory retains names and types only.
    let mut result = page(
        endpoint,
        output
            .resource_record_sets()
            .iter()
            .map(|row| json!({"Name":row.name(),"Type":row.r#type().as_str()}))
            .collect(),
        None,
    );
    result["IsTruncated"] = json!(output.is_truncated());
    result["NextRecordName"] = json!(output.next_record_name());
    result["NextRecordType"] = json!(output.next_record_type().map(|kind| kind.as_str()));
    result["NextRecordIdentifier"] = json!(output.next_record_identifier());
    Ok(result)
}

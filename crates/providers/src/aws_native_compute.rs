//! Compute inventory uses typed SDK responses and excludes tags, user data, and environment.
use crate::{
    aws_clients::AwsClients,
    aws_errors::sdk,
    aws_native::{cursor, date, page},
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
    if service == "autoscaling" {
        let client = clients
            .service(
                service,
                region,
                &job.settings,
                aws_sdk_autoscaling::Client::new,
            )
            .await?;
        let output = client
            .describe_auto_scaling_groups()
            .max_records(job.settings.page_size.min(100) as i32)
            .set_next_token(cursor(endpoint))
            .send()
            .await
            .map_err(sdk)?;
        let rows = output.auto_scaling_groups().iter().map(|row| {
            let instances: Vec<_> = row.instances().iter().map(|instance| json!({"HealthStatus":instance.health_status(),"LifecycleState":instance.lifecycle_state().map(|state| state.as_str())})).collect();
            json!({"AutoScalingGroupName":row.auto_scaling_group_name(),"DesiredCapacity":row.desired_capacity(),"CreatedTime":date(row.created_time()),"Instances":{"member":instances}})
        }).collect();
        return Ok(page(endpoint, rows, output.next_token()));
    }
    if service == "eks" {
        let client = clients
            .service(service, region, &job.settings, aws_sdk_eks::Client::new)
            .await?;
        if endpoint.id.starts_with("eks-detail/") {
            let name = url::Url::parse(&endpoint.url)
                .map_err(|_| Error::Malformed)?
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .ok_or(Error::Malformed)?
                .to_string();
            let output = client
                .describe_cluster()
                .name(name)
                .send()
                .await
                .map_err(sdk)?;
            let row = output.cluster().ok_or(Error::Missing)?;
            return Ok(
                json!({"cluster":{"name":row.name(),"arn":row.arn(),"status":row.status().map(|state| state.as_str())}}),
            );
        }
        let output = client
            .list_clusters()
            .max_results(job.settings.page_size.min(100) as i32)
            .set_next_token(cursor(endpoint))
            .send()
            .await
            .map_err(sdk)?;
        return Ok(page(
            endpoint,
            output.clusters().iter().map(|name| json!(name)).collect(),
            output.next_token(),
        ));
    }
    let client = clients
        .service("ec2", region, &job.settings, aws_sdk_ec2::Client::new)
        .await?;
    Ok(match action {
        "DescribeRegions" => {
            let output = client
                .describe_regions()
                .all_regions(true)
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.regions().iter().map(|row| json!({"regionName":row.region_name(),"optInStatus":row.opt_in_status()})).collect(), None)
        }
        "DescribeInstances" => {
            let output = client
                .describe_instances()
                .max_results(job.settings.page_size.max(5) as i32)
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            let rows = output.reservations().iter().map(|row| {
                let instances: Vec<_> = row.instances().iter().map(|instance| json!({"instanceId":instance.instance_id(),"instanceState":{"name":instance.state().and_then(|state| state.name()).map(|state| state.as_str())}})).collect();
                json!({"instancesSet":{"item":instances}})
            }).collect();
            page(endpoint, rows, output.next_token())
        }
        "DescribeInstanceStatus" => {
            let output = client
                .describe_instance_status()
                .include_all_instances(true)
                .max_results(job.settings.page_size.max(5) as i32)
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.instance_statuses().iter().map(|row| json!({"instanceId":row.instance_id(),"instanceStatus":{"status":row.instance_status().and_then(|status| status.status()).map(|status| status.as_str())}})).collect(), output.next_token())
        }
        "DescribeVolumes" => {
            let output = client
                .describe_volumes()
                .max_results(job.settings.page_size.max(5) as i32)
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.volumes().iter().map(|row| json!({"volumeId":row.volume_id(),"status":row.state().map(|state| state.as_str()),"encrypted":row.encrypted(),"size":row.size()})).collect(), output.next_token())
        }
        _ => return Err(Error::Forbidden),
    })
}

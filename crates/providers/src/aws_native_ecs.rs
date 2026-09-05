//! ECS inventory and batch runtime metadata exclude task overrides and environment values.
use crate::{
    aws_clients::AwsClients,
    aws_errors::sdk,
    aws_native::{cursor, date, page, required, strings},
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
        .service("ecs", region, &job.settings, aws_sdk_ecs::Client::new)
        .await?;
    Ok(match action {
        "ListClusters" => {
            let output = client
                .list_clusters()
                .max_results(job.settings.page_size.min(100) as i32)
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(
                endpoint,
                output.cluster_arns().iter().map(|arn| json!(arn)).collect(),
                output.next_token(),
            )
        }
        "ListServices" => {
            let output = client
                .list_services()
                .cluster(required(endpoint, "cluster")?)
                .max_results(job.settings.page_size.min(100) as i32)
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(
                endpoint,
                output.service_arns().iter().map(|arn| json!(arn)).collect(),
                output.next_token(),
            )
        }
        "ListTasks" => {
            let output = client
                .list_tasks()
                .cluster(required(endpoint, "cluster")?)
                .desired_status(aws_sdk_ecs::types::DesiredStatus::Running)
                .max_results(job.settings.page_size.min(100) as i32)
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(
                endpoint,
                output.task_arns().iter().map(|arn| json!(arn)).collect(),
                output.next_token(),
            )
        }
        "DescribeServices" => {
            let output = client
                .describe_services()
                .cluster(required(endpoint, "cluster")?)
                .set_services(Some(strings(endpoint, "services")?))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.services().iter().map(|row| json!({"serviceArn":row.service_arn(),"serviceName":row.service_name(),"desiredCount":row.desired_count(),"runningCount":row.running_count(),"status":row.status(),"createdAt":date(row.created_at())})).collect(), None)
        }
        "DescribeTasks" => {
            let output = client
                .describe_tasks()
                .cluster(required(endpoint, "cluster")?)
                .set_tasks(Some(strings(endpoint, "tasks")?))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.tasks().iter().map(task).collect(), None)
        }
        _ => return Err(Error::Forbidden),
    })
}
fn task(row: &aws_sdk_ecs::types::Task) -> Value {
    let containers: Vec<_> = row.containers().iter().map(|container| json!({"name":container.name(),"image":container.image(),"imageDigest":container.image_digest()})).collect();
    json!({"taskArn":row.task_arn(),"lastStatus":row.last_status(),"desiredStatus":row.desired_status(),"healthStatus":row.health_status().map(|state| state.as_str()),"createdAt":date(row.created_at()),"containers":containers})
}

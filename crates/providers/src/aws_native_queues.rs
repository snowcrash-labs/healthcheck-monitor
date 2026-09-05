//! Queue and event metadata through official SDKs; message operations are absent.
use crate::{
    aws_clients::AwsClients,
    aws_errors::sdk,
    aws_native::{arg, cursor, page, required, strings},
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
    match action {
        "ListQueues" | "GetQueueAttributes" => {
            let client = clients
                .service("sqs", region, &job.settings, aws_sdk_sqs::Client::new)
                .await?;
            if action == "ListQueues" {
                let output = client
                    .list_queues()
                    .max_results(job.settings.page_size as i32)
                    .set_next_token(cursor(endpoint))
                    .send()
                    .await
                    .map_err(sdk)?;
                Ok(page(
                    endpoint,
                    output.queue_urls().iter().map(|url| json!(url)).collect(),
                    output.next_token(),
                ))
            } else {
                let names: Vec<_> = strings(endpoint, "AttributeNames")?
                    .into_iter()
                    .map(|name| aws_sdk_sqs::types::QueueAttributeName::from(name.as_str()))
                    .collect();
                let output = client
                    .get_queue_attributes()
                    .queue_url(required(endpoint, "QueueUrl")?)
                    .set_attribute_names(Some(names))
                    .send()
                    .await
                    .map_err(sdk)?;
                let attributes: serde_json::Map<_, _> = output
                    .attributes()
                    .into_iter()
                    .flatten()
                    .map(|(key, value)| (key.as_str().to_owned(), json!(value)))
                    .collect();
                Ok(json!({"Attributes":attributes}))
            }
        }
        "ListTopics" => {
            let client = clients
                .service("sns", region, &job.settings, aws_sdk_sns::Client::new)
                .await?;
            let output = client
                .list_topics()
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            Ok(page(
                endpoint,
                output
                    .topics()
                    .iter()
                    .map(|topic| json!({"TopicArn":topic.topic_arn()}))
                    .collect(),
                output.next_token(),
            ))
        }
        "ListRules" => {
            let client = clients
                .service(
                    "events",
                    region,
                    &job.settings,
                    aws_sdk_eventbridge::Client::new,
                )
                .await?;
            let output = client
                .list_rules()
                .limit(job.settings.page_size.min(100) as i32)
                .set_event_bus_name(arg(endpoint, "EventBusName"))
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            Ok(page(endpoint, output.rules().iter().map(|rule| json!({"Name":rule.name(),"Arn":rule.arn(),"State":rule.state().map(|state| state.as_str())})).collect(), output.next_token()))
        }
        _ => Err(Error::Forbidden),
    }
}

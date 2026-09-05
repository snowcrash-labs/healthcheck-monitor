//! Function SDK responses discard environment, invocation payloads, and signed download URLs.
use crate::{
    aws_clients::AwsClients,
    aws_errors::sdk,
    aws_native::{cursor, page},
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
) -> Result<Value, Error> {
    let client = clients
        .service("lambda", region, &job.settings, aws_sdk_lambda::Client::new)
        .await?;
    let url = url::Url::parse(&endpoint.url).map_err(|_| Error::Malformed)?;
    let name = url
        .path()
        .strip_prefix("/2015-03-31/functions/")
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("");
    if name.is_empty() {
        let output = client
            .list_functions()
            .max_items(job.settings.page_size.min(50) as i32)
            .set_marker(cursor(endpoint))
            .send()
            .await
            .map_err(sdk)?;
        return Ok(page(
            endpoint,
            output.functions().iter().map(configuration).collect(),
            output.next_marker(),
        ));
    }
    if endpoint.id.starts_with("lambda-image/") {
        let output = client
            .get_function()
            .function_name(name)
            .send()
            .await
            .map_err(sdk)?;
        let config = output.configuration().ok_or(Error::Missing)?;
        return Ok(
            json!({"Configuration":configuration(config),"Code":{"ImageUri":output.code().and_then(|code| code.image_uri()),"ResolvedImageUri":output.code().and_then(|code| code.resolved_image_uri())}}),
        );
    }
    let output = client
        .get_function_configuration()
        .function_name(name)
        .send()
        .await
        .map_err(sdk)?;
    Ok(
        json!({"FunctionName":output.function_name(),"State":output.state().map(|state| state.as_str()),"PackageType":output.package_type().map(|kind| kind.as_str())}),
    )
}
fn configuration(row: &aws_sdk_lambda::types::FunctionConfiguration) -> Value {
    json!({"FunctionName":row.function_name(),"FunctionArn":row.function_arn(),"State":row.state().map(|state| state.as_str()),"PackageType":row.package_type().map(|kind| kind.as_str())})
}

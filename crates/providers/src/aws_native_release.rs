//! Native registry and release reads preserve provenance while discarding build payloads.
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
    service: &str,
    action: &str,
) -> Result<Value, Error> {
    if service == "ecr" {
        return registry(clients, endpoint, job, region, action).await;
    }
    if service == "codepipeline" {
        return pipeline(clients, endpoint, job, region, action).await;
    }
    let client = clients
        .service(
            "codebuild",
            region,
            &job.settings,
            aws_sdk_codebuild::Client::new,
        )
        .await?;
    Ok(match action {
        "ListBuilds" => {
            let output = client
                .list_builds()
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(
                endpoint,
                output.ids().iter().map(|id| json!(id)).collect(),
                output.next_token(),
            )
        }
        "BatchGetBuilds" => {
            let output = client
                .batch_get_builds()
                .set_ids(Some(strings(endpoint, "ids")?))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.builds().iter().map(|build| json!({"id":build.id(),"arn":build.arn(),"projectName":build.project_name(),"resolvedSourceVersion":build.resolved_source_version(),"buildStatus":build.build_status().map(|state| state.as_str()),"startTime":date(build.start_time())})).collect(), None)
        }
        _ => return Err(Error::Forbidden),
    })
}
async fn registry(
    clients: &AwsClients,
    endpoint: &Endpoint,
    job: &Job,
    region: &str,
    action: &str,
) -> Result<Value, Error> {
    let client = clients
        .service("ecr", region, &job.settings, aws_sdk_ecr::Client::new)
        .await?;
    Ok(match action {
        "DescribeRepositories" => {
            let output = client
                .describe_repositories()
                .max_results(job.settings.page_size as i32)
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.repositories().iter().map(|row| json!({"repositoryName":row.repository_name(),"repositoryArn":row.repository_arn(),"repositoryUri":row.repository_uri()})).collect(), output.next_token())
        }
        "DescribeImages" => {
            let output = client
                .describe_images()
                .repository_name(required(endpoint, "repositoryName")?)
                .max_results(job.settings.page_size as i32)
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.image_details().iter().map(|row| json!({"repositoryName":row.repository_name(),"imageDigest":row.image_digest(),"imageTags":row.image_tags(),"imageManifestMediaType":row.image_manifest_media_type(),"imagePushedAt":date(row.image_pushed_at())})).collect(), output.next_token())
        }
        "BatchGetImage" => {
            let ids = endpoint
                .body
                .as_ref()
                .and_then(|body| body.get("imageIds"))
                .and_then(Value::as_array)
                .ok_or(Error::Malformed)?;
            let ids: Result<Vec<_>, Error> = ids
                .iter()
                .map(|id| {
                    let digest = id
                        .get("imageDigest")
                        .and_then(Value::as_str)
                        .ok_or(Error::Malformed)?;
                    Ok(aws_sdk_ecr::types::ImageIdentifier::builder()
                        .image_digest(digest)
                        .build())
                })
                .collect();
            let output = client
                .batch_get_image()
                .repository_name(required(endpoint, "repositoryName")?)
                .set_image_ids(Some(ids?))
                .send()
                .await
                .map_err(sdk)?;
            let rows: Result<Vec<_>, Error> = output.images().iter().map(|image| {
                let raw: Value = serde_json::from_str(image.image_manifest().ok_or(Error::Malformed)?).map_err(|_| Error::Malformed)?;
                let children = raw.get("manifests").and_then(Value::as_array).ok_or(Error::Malformed)?;
                if children.len() > job.settings.max_series { return Err(Error::Limit); }
                let children: Vec<_> = children.iter().map(|child| json!({"digest":child.get("digest")})).collect();
                let manifest = json!({"schemaVersion":raw.get("schemaVersion"),"mediaType":raw.get("mediaType"),"manifests":children});
                Ok(json!({"imageId":{"imageDigest":image.image_id().and_then(|id| id.image_digest())},"imageManifest":serde_json::to_string(&manifest).map_err(|_| Error::Malformed)?}))
            }).collect();
            page(endpoint, rows?, None)
        }
        _ => return Err(Error::Forbidden),
    })
}
async fn pipeline(
    clients: &AwsClients,
    endpoint: &Endpoint,
    job: &Job,
    region: &str,
    action: &str,
) -> Result<Value, Error> {
    let client = clients
        .service(
            "codepipeline",
            region,
            &job.settings,
            aws_sdk_codepipeline::Client::new,
        )
        .await?;
    Ok(match action {
        "ListPipelines" => {
            let output = client
                .list_pipelines()
                .max_results(job.settings.page_size.min(1000) as i32)
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(
                endpoint,
                output
                    .pipelines()
                    .iter()
                    .map(|row| json!({"name":row.name()}))
                    .collect(),
                output.next_token(),
            )
        }
        "GetPipelineState" => {
            let output = client
                .get_pipeline_state()
                .name(required(endpoint, "name")?)
                .send()
                .await
                .map_err(sdk)?;
            json!({"pipelineName":output.pipeline_name()})
        }
        "ListPipelineExecutions" => {
            let output = client
                .list_pipeline_executions()
                .pipeline_name(required(endpoint, "pipelineName")?)
                .max_results(job.settings.page_size.min(100) as i32)
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            let rows = output.pipeline_execution_summaries().iter().map(|row| {
                let revisions: Vec<_> = row.source_revisions().iter().map(|revision| json!({"revisionId":revision.revision_id()})).collect();
                json!({"pipelineExecutionId":row.pipeline_execution_id(),"status":row.status().map(|state| state.as_str()),"startTime":date(row.start_time()),"sourceRevisions":revisions})
            }).collect();
            page(endpoint, rows, output.next_token())
        }
        _ => return Err(Error::Forbidden),
    })
}

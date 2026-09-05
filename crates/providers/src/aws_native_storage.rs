//! Storage SDKs read bucket controls and recovery metadata without object or backup contents.
use crate::{
    aws_clients::AwsClients,
    aws_errors::sdk,
    aws_native::{arg, cursor, date, page},
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
) -> Result<Value, Error> {
    if service == "backup" {
        return backups(clients, endpoint, job, region).await;
    }
    let client = clients
        .service("s3", region, &job.settings, aws_sdk_s3::Client::new)
        .await?;
    let url = url::Url::parse(&endpoint.url).map_err(|_| Error::Malformed)?;
    let bucket = url.path().trim_matches('/');
    if bucket.is_empty() {
        let output = client
            .list_buckets()
            .bucket_region(region)
            .max_buckets(job.settings.page_size as i32)
            .set_continuation_token(cursor(endpoint))
            .send()
            .await
            .map_err(sdk)?;
        return Ok(page(
            endpoint,
            output
                .buckets()
                .iter()
                .map(|row| json!({"Name":row.name(),"CreationDate":date(row.creation_date())}))
                .collect(),
            output.continuation_token(),
        ));
    }
    Ok(match url.query().unwrap_or("") {
        "encryption" => {
            let output = client
                .get_bucket_encryption()
                .bucket(bucket)
                .send()
                .await
                .map_err(sdk)?;
            let config = output
                .server_side_encryption_configuration()
                .ok_or(Error::Missing)?;
            let rules: Vec<_> = config.rules().iter().map(|rule| json!({"ApplyServerSideEncryptionByDefault":{"SSEAlgorithm":rule.apply_server_side_encryption_by_default().map(|default| default.sse_algorithm().as_str())}})).collect();
            json!({"Rule":rules})
        }
        "versioning" => {
            let output = client
                .get_bucket_versioning()
                .bucket(bucket)
                .send()
                .await
                .map_err(sdk)?;
            json!({"Status":output.status().map(|status| status.as_str())})
        }
        "replication" => {
            let output = client
                .get_bucket_replication()
                .bucket(bucket)
                .send()
                .await
                .map_err(sdk)?;
            let config = output.replication_configuration().ok_or(Error::Missing)?;
            json!({"configured":!config.rules().is_empty()})
        }
        "publicAccessBlock" => {
            let output = client
                .get_public_access_block()
                .bucket(bucket)
                .send()
                .await
                .map_err(sdk)?;
            let config = output
                .public_access_block_configuration()
                .ok_or(Error::Missing)?;
            json!({"BlockPublicAcls":config.block_public_acls(),"IgnorePublicAcls":config.ignore_public_acls(),"BlockPublicPolicy":config.block_public_policy(),"RestrictPublicBuckets":config.restrict_public_buckets()})
        }
        _ => return Err(Error::Forbidden),
    })
}
async fn backups(
    clients: &AwsClients,
    endpoint: &Endpoint,
    job: &Job,
    region: &str,
) -> Result<Value, Error> {
    let client = clients
        .service("backup", region, &job.settings, aws_sdk_backup::Client::new)
        .await?;
    let url = url::Url::parse(&endpoint.url).map_err(|_| Error::Malformed)?;
    if endpoint.id.starts_with("recovery-points/") {
        let name = url
            .path()
            .strip_prefix("/backup-vaults/")
            .and_then(|path| path.strip_suffix("/recovery-points"))
            .ok_or(Error::Malformed)?;
        let output = client
            .list_recovery_points_by_backup_vault()
            .backup_vault_name(name)
            .max_results(job.settings.page_size as i32)
            .set_next_token(cursor(endpoint))
            .send()
            .await
            .map_err(sdk)?;
        let rows = output.recovery_points().iter().map(|row| json!({"RecoveryPointArn":row.recovery_point_arn(),"ResourceArn":row.resource_arn(),"Status":row.status().map(|state| state.as_str()),"CompletionDate":date(row.completion_date()),"CreationDate":date(row.creation_date()),"Lifecycle":{"DeleteAfterDays":row.lifecycle().and_then(|lifecycle| lifecycle.delete_after_days())}})).collect();
        return Ok(page(endpoint, rows, output.next_token()));
    }
    let output = client
        .list_backup_vaults()
        .max_results(job.settings.page_size as i32)
        .set_next_token(arg(endpoint, "nextToken"))
        .send()
        .await
        .map_err(sdk)?;
    Ok(page(endpoint, output.backup_vault_list().iter().map(|row| json!({"BackupVaultName":row.backup_vault_name(),"BackupVaultArn":row.backup_vault_arn()})).collect(), output.next_token()))
}

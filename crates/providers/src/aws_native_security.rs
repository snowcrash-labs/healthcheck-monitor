//! KMS, certificate, and secret metadata use native SDKs without value-access operations.
use crate::{
    aws_clients::AwsClients,
    aws_errors::sdk,
    aws_native::{cursor, date, page, required},
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
    if service == "acm" {
        return certificates(clients, endpoint, job, region, action).await;
    }
    if service == "secretsmanager" {
        return secrets(clients, endpoint, job, region, action).await;
    }
    let client = clients
        .service("kms", region, &job.settings, aws_sdk_kms::Client::new)
        .await?;
    Ok(match action {
        "ListKeys" => {
            let output = client
                .list_keys()
                .limit(job.settings.page_size as i32)
                .set_marker(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(
                endpoint,
                output
                    .keys()
                    .iter()
                    .map(|row| json!({"KeyId":row.key_id(),"KeyArn":row.key_arn()}))
                    .collect(),
                output.next_marker(),
            )
        }
        "DescribeKey" => {
            let output = client
                .describe_key()
                .key_id(required(endpoint, "KeyId")?)
                .send()
                .await
                .map_err(sdk)?;
            let row = output.key_metadata().ok_or(Error::Missing)?;
            json!({"KeyMetadata":{"KeyId":row.key_id(),"Arn":row.arn(),"Enabled":row.enabled(),"KeyState":row.key_state().map(|state| state.as_str()),"KeyUsage":row.key_usage().map(|usage| usage.as_str()),"ValidTo":date(row.valid_to())}})
        }
        _ => return Err(Error::Forbidden),
    })
}
async fn certificates(
    clients: &AwsClients,
    endpoint: &Endpoint,
    job: &Job,
    region: &str,
    action: &str,
) -> Result<Value, Error> {
    let client = clients
        .service("acm", region, &job.settings, aws_sdk_acm::Client::new)
        .await?;
    Ok(match action {
        "ListCertificates" => {
            let output = client
                .list_certificates()
                .max_items(job.settings.page_size as i32)
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.certificate_summary_list().iter().map(|row| json!({"CertificateArn":row.certificate_arn(),"DomainName":row.domain_name(),"Status":row.status().map(|state| state.as_str())})).collect(), output.next_token())
        }
        "DescribeCertificate" => {
            let output = client
                .describe_certificate()
                .certificate_arn(required(endpoint, "CertificateArn")?)
                .send()
                .await
                .map_err(sdk)?;
            let row = output.certificate().ok_or(Error::Missing)?;
            json!({"Certificate":{"CertificateArn":row.certificate_arn(),"Status":row.status().map(|state| state.as_str()),"NotAfter":date(row.not_after())}})
        }
        _ => return Err(Error::Forbidden),
    })
}
async fn secrets(
    clients: &AwsClients,
    endpoint: &Endpoint,
    job: &Job,
    region: &str,
    action: &str,
) -> Result<Value, Error> {
    let client = clients
        .service(
            "secretsmanager",
            region,
            &job.settings,
            aws_sdk_secretsmanager::Client::new,
        )
        .await?;
    Ok(match action {
        "ListSecrets" => {
            let output = client
                .list_secrets()
                .max_results(job.settings.page_size.min(100) as i32)
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.secret_list().iter().map(|row| json!({"Name":row.name(),"ARN":row.arn(),"RotationEnabled":row.rotation_enabled()})).collect(), output.next_token())
        }
        "ListSecretVersionIds" => {
            let output = client
                .list_secret_version_ids()
                .secret_id(required(endpoint, "SecretId")?)
                .max_results(job.settings.page_size.min(100) as i32)
                .set_next_token(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.versions().iter().map(|row| json!({"VersionId":row.version_id(),"VersionStages":row.version_stages(),"CreatedDate":date(row.created_date())})).collect(), output.next_token())
        }
        _ => return Err(Error::Forbidden),
    })
}

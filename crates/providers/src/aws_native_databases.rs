//! Database and cache SDK reads retain operational metadata without connecting to application data.
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
    if service == "dynamodb" {
        return dynamo(clients, endpoint, job, region, action).await;
    }
    if service == "elasticache" {
        return cache(clients, endpoint, job, region, action).await;
    }
    let client = clients
        .service("rds", region, &job.settings, aws_sdk_rds::Client::new)
        .await?;
    Ok(match action {
        "DescribeDBInstances" => {
            let output = client
                .describe_db_instances()
                .max_records(job.settings.page_size.clamp(20, 100) as i32)
                .set_marker(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.db_instances().iter().map(|row| json!({"DBInstanceIdentifier":row.db_instance_identifier(),"DBInstanceStatus":row.db_instance_status(),"BackupRetentionPeriod":row.backup_retention_period(),"StorageEncrypted":row.storage_encrypted(),"AllocatedStorage":row.allocated_storage(),"PreferredMaintenanceWindow":row.preferred_maintenance_window(),"PendingModifiedValues":row.pending_modified_values().filter(|pending| **pending != aws_sdk_rds::types::PendingModifiedValues::builder().build()).map(|_| json!({"pending":true}))})).collect(), output.marker())
        }
        "DescribeDBClusters" => {
            let output = client
                .describe_db_clusters()
                .max_records(job.settings.page_size.clamp(20, 100) as i32)
                .set_marker(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.db_clusters().iter().map(|row| json!({"DBClusterIdentifier":row.db_cluster_identifier(),"Status":row.status(),"BackupRetentionPeriod":row.backup_retention_period(),"StorageEncrypted":row.storage_encrypted(),"PreferredMaintenanceWindow":row.preferred_maintenance_window(),"PendingModifiedValues":row.pending_modified_values().filter(|pending| **pending != aws_sdk_rds::types::ClusterPendingModifiedValues::builder().build()).map(|_| json!({"pending":true}))})).collect(), output.marker())
        }
        _ => return Err(Error::Forbidden),
    })
}
async fn cache(
    clients: &AwsClients,
    endpoint: &Endpoint,
    job: &Job,
    region: &str,
    action: &str,
) -> Result<Value, Error> {
    let client = clients
        .service(
            "elasticache",
            region,
            &job.settings,
            aws_sdk_elasticache::Client::new,
        )
        .await?;
    Ok(match action {
        "DescribeCacheClusters" => {
            let output = client
                .describe_cache_clusters()
                .max_records(job.settings.page_size.clamp(20, 100) as i32)
                .set_marker(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.cache_clusters().iter().map(|row| json!({"CacheClusterId":row.cache_cluster_id(),"CacheClusterStatus":row.cache_cluster_status(),"PreferredMaintenanceWindow":row.preferred_maintenance_window(),"PendingModifiedValues":row.pending_modified_values().filter(|pending| **pending != aws_sdk_elasticache::types::PendingModifiedValues::builder().build()).map(|_| json!({"pending":true}))})).collect(), output.marker())
        }
        "DescribeReplicationGroups" => {
            let output = client
                .describe_replication_groups()
                .max_records(job.settings.page_size.clamp(20, 100) as i32)
                .set_marker(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(endpoint, output.replication_groups().iter().map(|row| json!({"ReplicationGroupId":row.replication_group_id(),"Status":row.status(),"AtRestEncryptionEnabled":row.at_rest_encryption_enabled(),"PendingModifiedValues":row.pending_modified_values().filter(|pending| **pending != aws_sdk_elasticache::types::ReplicationGroupPendingModifiedValues::builder().build()).map(|_| json!({"pending":true}))})).collect(), output.marker())
        }
        _ => return Err(Error::Forbidden),
    })
}
async fn dynamo(
    clients: &AwsClients,
    endpoint: &Endpoint,
    job: &Job,
    region: &str,
    action: &str,
) -> Result<Value, Error> {
    let client = clients
        .service(
            "dynamodb",
            region,
            &job.settings,
            aws_sdk_dynamodb::Client::new,
        )
        .await?;
    Ok(match action {
        "ListTables" => {
            let output = client
                .list_tables()
                .limit(job.settings.page_size.min(100) as i32)
                .set_exclusive_start_table_name(cursor(endpoint))
                .send()
                .await
                .map_err(sdk)?;
            page(
                endpoint,
                output
                    .table_names()
                    .iter()
                    .map(|name| json!(name))
                    .collect(),
                output.last_evaluated_table_name(),
            )
        }
        "DescribeTable" => {
            let output = client
                .describe_table()
                .table_name(required(endpoint, "TableName")?)
                .send()
                .await
                .map_err(sdk)?;
            let row = output.table().ok_or(Error::Missing)?;
            json!({"Table":{"TableName":row.table_name(),"TableStatus":row.table_status().map(|state| state.as_str()),"TableSizeBytes":row.table_size_bytes()}})
        }
        "DescribeContinuousBackups" => {
            let output = client
                .describe_continuous_backups()
                .table_name(required(endpoint, "TableName")?)
                .send()
                .await
                .map_err(sdk)?;
            let row = output
                .continuous_backups_description()
                .ok_or(Error::Missing)?;
            let recovery = row.point_in_time_recovery_description();
            json!({"ContinuousBackupsDescription":{"PointInTimeRecoveryDescription":{"PointInTimeRecoveryStatus":recovery.and_then(|row| row.point_in_time_recovery_status()).map(|state| state.as_str()),"LatestRestorableDateTime":date(recovery.and_then(|row| row.latest_restorable_date_time()))}}})
        }
        _ => return Err(Error::Forbidden),
    })
}

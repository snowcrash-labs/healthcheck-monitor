//! Human-readable metric resource metadata excludes arbitrary provider dimensions.
use std::collections::BTreeMap;
pub fn label_path<'a>(labels: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let allowed = [
        "project_id",
        "resource_container",
        "location",
        "zone",
        "cluster_name",
        "namespace_name",
        "pod_name",
        "container_name",
        "instance_id",
        "database_id",
        "subscription_id",
        "bucket_name",
        "service_name",
        "memory_type",
        "memory_state",
        "role",
        "node_id",
        "shard_id",
        "response_code",
        "response_code_class",
        "InstanceId",
        "VolumeId",
        "AutoScalingGroupName",
        "DBInstanceIdentifier",
        "DBClusterIdentifier",
        "CacheClusterId",
        "CacheNodeId",
        "QueueName",
        "FunctionName",
        "ServiceName",
        "ClusterName",
        "LoadBalancer",
        "TargetGroup",
        "TableName",
        "BucketName",
        "StreamName",
        "RuleName",
        "EventBusName",
        "ShardId",
    ];
    let labels: BTreeMap<_, _> = labels
        .filter(|(key, _)| allowed.contains(key))
        .take(32)
        .collect();
    labels
        .into_iter()
        .map(|(key, value)| {
            format!(
                "{key}:{}",
                monitor_integrations::projection::identity(value)
            )
        })
        .collect::<Vec<_>>()
        .join("/")
}

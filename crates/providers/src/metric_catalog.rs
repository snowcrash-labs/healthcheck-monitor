//! Automatic metric presets; business thresholds remain operator supplied.
use monitor_core::config::types::{Aggregation, MetricQuery};
use std::collections::BTreeMap;
pub fn gcp() -> Vec<MetricQuery> {
    GCP.iter()
        .map(|(name, namespace, metric, capacity)| MetricQuery {
            aggregation: if capacity.is_some() {
                Aggregation::Minimum
            } else if matches!(
                *name,
                "pubsub-delivery"
                    | "pubsub-dead-letters"
                    | "pubsub-published"
                    | "run-requests"
                    | "storage-requests"
            ) {
                Aggregation::Sum
            } else {
                Aggregation::Latest
            },
            name: (*name).into(),
            namespace: (*namespace).into(),
            metric: (*metric).into(),
            resource: (*name).into(),
            dimensions: BTreeMap::new(),
            capacity: *capacity,
            warning: (*name == "pubsub-dead-letters").then_some(1.0),
            error: None,
        })
        .collect()
}
const GCP: &[(&str, &str, &str, Option<f64>)] = &[
    (
        "redis-cluster-memory",
        "redis.googleapis.com/Cluster",
        "redis.googleapis.com/cluster/memory/maximum_utilization",
        Some(1.0),
    ),
    (
        "redis-cluster-cpu",
        "redis.googleapis.com/Cluster",
        "redis.googleapis.com/cluster/cpu/maximum_utilization",
        Some(1.0),
    ),
    (
        "redis-cluster-headroom",
        "redis.googleapis.com/Cluster",
        "redis.googleapis.com/cluster/memory/size",
        None,
    ),
    (
        "redis-cluster-replication-lag",
        "redis.googleapis.com/Cluster",
        "redis.googleapis.com/cluster/replication/maximum_ack_lag",
        None,
    ),
    (
        "pubsub-published",
        "pubsub_topic",
        "pubsub.googleapis.com/topic/send_message_operation_count",
        None,
    ),
    (
        "pubsub-dead-letters",
        "pubsub_subscription",
        "pubsub.googleapis.com/subscription/dead_letter_message_count",
        None,
    ),
    (
        "run-latency-ms",
        "cloud_run_revision",
        "run.googleapis.com/request_latencies",
        None,
    ),
    (
        "valkey-memory",
        "memorystore.googleapis.com/Instance",
        "memorystore.googleapis.com/instance/memory/maximum_utilization",
        Some(1.0),
    ),
    (
        "valkey-cpu",
        "memorystore.googleapis.com/Instance",
        "memorystore.googleapis.com/instance/cpu/maximum_utilization",
        Some(1.0),
    ),
    (
        "valkey-headroom",
        "memorystore.googleapis.com/Instance",
        "memorystore.googleapis.com/instance/memory/size",
        None,
    ),
    (
        "valkey-evictions",
        "memorystore.googleapis.com/Instance",
        "memorystore.googleapis.com/instance/stats/total_evicted_keys_count",
        None,
    ),
    (
        "valkey-replication-lag",
        "memorystore.googleapis.com/Instance",
        "memorystore.googleapis.com/instance/replication/average_ack_lag",
        None,
    ),
    (
        "node-cpu",
        "k8s_node",
        "kubernetes.io/node/cpu/allocatable_utilization",
        Some(1.0),
    ),
    (
        "container-cpu",
        "k8s_container",
        "kubernetes.io/container/cpu/limit_utilization",
        Some(1.0),
    ),
    (
        "container-memory",
        "k8s_container",
        "kubernetes.io/container/memory/limit_utilization",
        Some(1.0),
    ),
    (
        "compute-cpu",
        "gce_instance",
        "compute.googleapis.com/instance/cpu/utilization",
        Some(1.0),
    ),
    (
        "sql-cpu",
        "cloudsql_database",
        "cloudsql.googleapis.com/database/cpu/utilization",
        Some(1.0),
    ),
    (
        "sql-memory",
        "cloudsql_database",
        "cloudsql.googleapis.com/database/memory/utilization",
        Some(1.0),
    ),
    (
        "sql-connections",
        "cloudsql_database",
        "cloudsql.googleapis.com/database/postgresql/num_backends",
        None,
    ),
    (
        "sql-replica-lag",
        "cloudsql_database",
        "cloudsql.googleapis.com/database/replication/replica_lag",
        None,
    ),
    (
        "redis-memory",
        "redis_instance",
        "redis.googleapis.com/stats/memory/usage_ratio",
        Some(1.0),
    ),
    (
        "redis-evictions",
        "redis_instance",
        "redis.googleapis.com/stats/evicted_keys",
        None,
    ),
    (
        "pubsub-backlog",
        "pubsub_subscription",
        "pubsub.googleapis.com/subscription/num_undelivered_messages",
        None,
    ),
    (
        "pubsub-age",
        "pubsub_subscription",
        "pubsub.googleapis.com/subscription/oldest_unacked_message_age",
        None,
    ),
    (
        "pubsub-delivery",
        "pubsub_subscription",
        "pubsub.googleapis.com/subscription/push_request_count",
        None,
    ),
    (
        "run-requests",
        "cloud_run_revision",
        "run.googleapis.com/request_count",
        None,
    ),
    (
        "storage-requests",
        "gcs_bucket",
        "storage.googleapis.com/api/request_count",
        None,
    ),
];
pub const AWS_NAMESPACES: &[&str] = &[
    "AWS/EC2",
    "AWS/EBS",
    "AWS/AutoScaling",
    "AWS/EKS",
    "AWS/ECS",
    "AWS/Lambda",
    "AWS/ApplicationELB",
    "AWS/NetworkELB",
    "AWS/CloudFront",
    "AWS/RDS",
    "AWS/ElastiCache",
    "AWS/DynamoDB",
    "AWS/S3",
    "AWS/SQS",
    "AWS/SNS",
    "AWS/Events",
    "AWS/CodeBuild",
];
pub fn percent_metric(name: &str) -> bool {
    matches!(
        name,
        "CPUUtilization"
            | "EngineCPUUtilization"
            | "DatabaseMemoryUsagePercentage"
            | "MemoryUtilization"
            | "PercentIOLimit"
            | "DatabaseConnectionsUtilization"
            | "Percentage CPU"
            | "cpu_percent"
            | "memory_percent"
            | "usedmemorypercentage"
            | "serverLoad"
            | "CpuPercentage"
            | "MemoryPercentage"
            | "dtu_consumption_percent"
            | "storage_percent"
            | "connection_percent"
            | "workers_percent"
            | "sessions_percent"
            | "log_write_percent"
            | "physical_data_read_percent"
            | "NormalizedRUConsumption"
    )
}

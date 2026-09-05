//! Automatic metric presets; business thresholds remain operator supplied.
use monitor_core::config::types::MetricQuery;
use std::collections::BTreeMap;
pub fn gcp() -> Vec<MetricQuery> {
    GCP.iter()
        .map(|(name, namespace, metric, capacity)| MetricQuery {
            aggregation: Default::default(),
            name: (*name).into(),
            namespace: (*namespace).into(),
            metric: (*metric).into(),
            resource: (*name).into(),
            dimensions: BTreeMap::new(),
            capacity: *capacity,
            warning: None,
            error: None,
        })
        .collect()
}
const GCP: &[(&str, &str, &str, Option<f64>)] = &[
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
    )
}

//! Native cloud monitoring adapters.
mod advertisements;
pub mod auth;
pub mod aws;
mod aws_catalog;
mod aws_clients;
mod aws_metric_discovery;
pub mod aws_metrics;
#[cfg(test)]
mod aws_tests;
pub mod aws_transport;
pub mod azure;
mod cloud_logs;
pub mod common;
pub mod details;
pub mod discovery;
mod endpoint_scan;
pub mod gcp;
mod gcp_catalog;
pub mod inventory_cache;
pub mod metric_catalog;
pub mod metric_window;
pub mod metrics;
pub mod resource_projection;
pub mod router;
mod quotas;
mod routing;
mod routing_edge;

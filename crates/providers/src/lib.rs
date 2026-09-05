//! Native cloud monitoring adapters.
pub mod auth;
pub mod aws;
pub mod aws_metrics;
pub mod aws_transport;
pub mod azure;
pub mod common;
pub mod details;
pub mod discovery;
pub mod gcp;
mod gcp_catalog;
pub mod metric_catalog;
pub mod metrics;
pub mod resource_projection;
pub mod router;
mod routing;

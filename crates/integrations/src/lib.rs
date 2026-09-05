//! Native shared integrations with bounded transport and metadata projection.
pub mod changes;
pub mod endpoint;
pub mod github;
mod github_commits;
mod kube_auth;
pub mod kube_links;
pub mod kube_projection;
pub mod kubernetes;
pub mod log_dedup;
pub mod log_window;
pub mod logs;
pub mod nats;
mod nats_collect;
pub mod process;
#[cfg(test)]
mod process_tests;
pub mod projection;
pub mod queues;
mod read_policy;
pub mod transport;
pub mod xml;

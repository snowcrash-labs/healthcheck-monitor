//! Native shared integrations with bounded transport and metadata projection.
pub mod admission;
pub mod changes;
mod dns;
pub mod endpoint;
pub mod github;
mod github_commits;
mod github_requests;
pub mod http_pool;
mod kube_auth;
pub mod kube_collect;
mod kube_conditions;
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
mod worker_index;
pub mod xml;

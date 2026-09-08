//! Read-only HTTP access to an autonomous monitoring runtime.
mod api;
mod body;
mod build_resources;
mod build_view;
pub mod bus;
mod checks_api;
pub mod config;
mod console_links;
mod cost_api;
mod cost_reader;
mod cost_worker;
mod encoding;
mod events;
mod facts;
mod facts_metadata;
mod history_api;
mod iap;
mod listener;
mod lists;
mod log_links;
mod overview;
mod pages;
mod problem_groups;
mod query_api;
mod query_availability;
mod query_cursor;
mod query_deployment;
mod query_deployment_pages;
mod query_observations;
mod query_openapi;
mod query_projection;
mod query_publish;
mod query_recorder;
#[cfg(test)]
mod query_tests;
mod resource_evidence;
mod resource_rows;
mod response;
mod security;
mod serve;
mod static_files;
mod view;
pub use serve::serve;
#[cfg(test)]
mod api_tests;
#[cfg(test)]
mod console_tests;
#[cfg(test)]
mod diagnostic_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tls_tests;
#[cfg(test)]
mod view_tests;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid server configuration")]
    Configuration,
    #[error("server I/O failed")]
    Io(#[from] std::io::Error),
    #[error("monitoring runtime failed")]
    Runtime(#[from] monitor_core::error::Error),
    #[error("history configuration unavailable")]
    History,
    #[error("server task failed")]
    Task,
}

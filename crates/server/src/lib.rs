//! Read-only HTTP access to an autonomous monitoring runtime.
mod api;
mod body;
mod build_view;
pub mod bus;
pub mod config;
mod events;
mod facts;
mod facts_metadata;
mod history_api;
mod listener;
mod lists;
mod overview;
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

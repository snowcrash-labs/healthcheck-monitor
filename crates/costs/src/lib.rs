//! Billing contracts keep exact amounts and source coverage independent of health evidence.
pub mod aggregate;
pub mod config;
pub mod error;
pub mod gcp_query;
pub mod model;
pub mod query;
#[cfg(test)]
mod tests;

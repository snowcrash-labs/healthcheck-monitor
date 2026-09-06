//! Finite history retention, connection, and journal admission settings.
use crate::error::Error;
use monitor_core::config::duration::Span;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub database_url_env: String,
    pub connections: u32,
    pub retention: Span,
    pub event_rows: i64,
    pub run_rows: i64,
    pub gap_rows: i64,
    pub configuration_rows: i64,
    pub diagnostic_rows: i64,
    pub queue_bytes: usize,
    pub queue_batches: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            database_url_env: "HEALTHCHECK_DATABASE_URL".into(),
            connections: 2,
            retention: Span(7 * 86400),
            event_rows: 100000,
            run_rows: 1000000,
            gap_rows: 1000,
            configuration_rows: 32,
            diagnostic_rows: 250000,
            queue_bytes: 16 * 1024 * 1024,
            queue_batches: 64,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<(), Error> {
        if self.database_url_env.is_empty()
            || self.database_url_env.len() > 128
            || !self
                .database_url_env
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
            || !(2..=8).contains(&self.connections)
            || !(60..=31 * 86400).contains(&self.retention.0)
            || !(1..=100000).contains(&self.event_rows)
            || !(1..=1000000).contains(&self.run_rows)
            || !(1..=10000).contains(&self.gap_rows)
            || !(1..=1024).contains(&self.configuration_rows)
            || !(1..=1000000).contains(&self.diagnostic_rows)
            || !(65536..=64 * 1024 * 1024).contains(&self.queue_bytes)
            || !(1..=256).contains(&self.queue_batches)
        {
            return Err(Error::Configuration);
        }
        Ok(())
    }
}

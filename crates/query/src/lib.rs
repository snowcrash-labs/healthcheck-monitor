//! Shared, allowlisted monitoring query contracts independent of collectors and HTTP frameworks.
pub mod assessment;
#[cfg(test)]
mod assessment_tests;
pub mod enums;
pub mod filter;
pub mod matching;
pub mod record;
pub mod release_assessment;
pub mod response;
#[cfg(test)]
mod tests;

/// Invalid requests and records never include untrusted values in their error messages.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid query filter or time window")]
    Filter,
    #[error("invalid or mismatched pagination cursor")]
    Cursor,
    #[error("diagnostic record exceeds its field bounds")]
    Record,
}

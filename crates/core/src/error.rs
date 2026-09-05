//! Errors safe to display without provider payloads or credentials.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid configuration: {0}")]
    Config(String),
    #[error("invalid evidence")]
    Evidence,
    #[error("configured resource capacity exhausted")]
    Capacity,
    #[error("filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("evidence encoding failed")]
    Json(#[from] serde_json::Error),
    #[error("another writer holds the evidence lock")]
    Locked,
}
impl Error {
    /// Only transient filesystem failures can be retried by persistence.
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Io(_))
    }
}

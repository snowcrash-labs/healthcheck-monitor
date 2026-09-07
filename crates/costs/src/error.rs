//! Public billing errors contain no provider payloads, account names, or credential details.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid billing configuration")]
    Configuration,
    #[error("invalid billing query")]
    Query,
    #[error("invalid or overflowing cost amount")]
    Amount,
    #[error("billing input is incomplete or exceeds its bound")]
    Limit,
    #[error("billing source returned an invalid record")]
    Record,
    #[error("billing revision changed; refresh the view")]
    Revision,
}

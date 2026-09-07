//! Fixed errors prevent OAuth responses, headers, and credentials from entering diagnostics.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid connection profile; configure the desktop OAuth client and HTTPS monitor URL")]
    Configuration,
    #[error("Google sign-in required; run healthcheck-connect login")]
    LoginRequired,
    #[error("Google identity lacks monitoring access")]
    Forbidden,
    #[error(
        "credential store unavailable; unlock the system credential store or explicitly select file storage"
    )]
    Credentials,
    #[error("Google authorization callback was cancelled, invalid, or timed out")]
    Callback,
    #[error("network operation failed or exceeded its deadline")]
    Network,
    #[error("monitor is unavailable or busy; retry later")]
    Unavailable,
    #[error("query parameters are invalid")]
    Query,
    #[error("response failed schema or size validation")]
    Response,
    #[error("MCP transport failed")]
    Transport,
}
impl Error {
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Network | Self::Unavailable)
    }
}

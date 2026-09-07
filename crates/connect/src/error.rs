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
    /// An inaccessible credential store must not be reported as missing Google authorization.
    pub fn credential_read(error: keyring::Error) -> Self {
        match error {
            keyring::Error::NoEntry => Self::LoginRequired,
            _ => Self::Credentials,
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(self, Self::Network | Self::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn only_missing_credentials_require_google_login() {
        assert!(matches!(
            Error::credential_read(keyring::Error::NoEntry),
            Error::LoginRequired
        ));
        for error in [
            keyring::Error::NoStorageAccess(Box::new(std::io::Error::from(
                std::io::ErrorKind::PermissionDenied,
            ))),
            keyring::Error::PlatformFailure(Box::new(std::io::Error::from(
                std::io::ErrorKind::PermissionDenied,
            ))),
            keyring::Error::NoDefaultStore,
            keyring::Error::BadEncoding(b"private credential bytes".to_vec()),
        ] {
            let mapped = Error::credential_read(error);
            assert!(matches!(mapped, Error::Credentials));
            assert!(!mapped.to_string().contains("private credential bytes"));
        }
    }
}

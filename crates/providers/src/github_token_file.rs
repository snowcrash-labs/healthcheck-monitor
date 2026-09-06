//! Read protected token files without exposing credential contents or allowing unbounded reads.
use monitor_integrations::transport::Error;
use std::{path::Path, time::Duration};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

pub(super) async fn read(
    path: &Path,
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<String, Error> {
    let read = async {
        let mut bytes = Vec::new();
        tokio::fs::File::open(path)
            .await
            .map_err(|_| Error::Authentication)?
            .take(16385)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| Error::Authentication)?;
        parse(&bytes)
    };
    tokio::select! {
        _ = cancel.cancelled() => Err(Error::Cancelled),
        result = tokio::time::timeout(timeout, read) => result.map_err(|_| Error::Timeout)?,
    }
}
fn parse(bytes: &[u8]) -> Result<String, Error> {
    if bytes.len() > 16384 {
        return Err(Error::Limit);
    }
    let token = std::str::from_utf8(bytes)
        .map_err(|_| Error::Authentication)?
        .trim();
    if token.is_empty()
        || !token.is_ascii()
        || token
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return Err(Error::Authentication);
    }
    Ok(token.into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_files_accept_a_final_newline_and_reject_invalid_or_oversized_values()
    -> Result<(), Error> {
        assert_eq!(parse(b"synthetic-token\n")?, "synthetic-token");
        for bytes in [
            b"\n".as_slice(),
            b"token\nsecond",
            b"token\0",
            b"token value",
            &[0xff],
        ] {
            assert!(parse(bytes).is_err());
        }
        assert!(matches!(parse(&vec![b'x'; 16385]), Err(Error::Limit)));
        Ok(())
    }
}

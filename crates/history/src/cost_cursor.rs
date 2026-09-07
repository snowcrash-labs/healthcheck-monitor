//! Billing cursors bind the publication, query selection, and row position.
use crate::error::Error;
use sha2::{Digest, Sha256};
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    offset: usize,
    revision: String,
    fingerprint: String,
}
pub(crate) fn decode_cursor(value: Option<&str>, revision: &str, fingerprint: &str) -> Result<usize, Error> {
    let Some(value) = value else {
        return Ok(0);
    };
    let cursor: Cursor = serde_json::from_str(value).map_err(|_| Error::Record)?;
    if cursor.offset > 500_000 || cursor.revision != revision || cursor.fingerprint != fingerprint {
        return Err(Error::Revision);
    }
    Ok(cursor.offset)
}
pub(crate) fn encode_cursor(offset: usize, revision: &str, fingerprint: &str) -> Result<String, Error> {
    serde_json::to_string(&Cursor {
        offset,
        revision: revision.into(),
        fingerprint: fingerprint.into(),
    })
    .map_err(|_| Error::Record)
}

pub(crate) fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

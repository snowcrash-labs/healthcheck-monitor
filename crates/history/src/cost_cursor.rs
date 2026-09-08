//! Billing cursors bind publication, filters, and the ordered amount/key boundary.
use crate::error::Error;
use monitor_costs::{aggregate::Contributor, model::Amount};
use sha2::{Digest, Sha256};
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    pub amount: Amount,
    pub key: String,
    revision: String,
    fingerprint: String,
}
pub(crate) fn decode_cursor(
    value: Option<&str>,
    revision: &str,
    fingerprint: &str,
) -> Result<Option<Cursor>, Error> {
    let Some(value) = value else {
        return Ok(None);
    };
    let cursor: Cursor = serde_json::from_str(value).map_err(|_| Error::Record)?;
    if cursor.key.len() > 4096 || cursor.revision != revision || cursor.fingerprint != fingerprint {
        return Err(Error::Revision);
    }
    Ok(Some(cursor))
}
pub(crate) fn encode_cursor(
    row: &Contributor,
    revision: &str,
    fingerprint: &str,
) -> Result<String, Error> {
    serde_json::to_string(&Cursor {
        amount: row.amount.clone(),
        key: row.key.clone(),
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

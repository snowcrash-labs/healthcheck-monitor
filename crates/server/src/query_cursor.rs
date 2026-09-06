//! Cursors bind selection and absolute window without retaining per-client state.
use crate::response::ApiError;
use chrono::{DateTime, Utc};
use monitor_history::types::Id;
use monitor_query::filter::{Filter, Window};
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8,
    fingerprint: String,
    window: Window,
    at: DateTime<Utc>,
    id: Id,
}
fn fingerprint(filter: &Filter, endpoint: &str) -> Result<String, ApiError> {
    let mut filter = filter.clone();
    filter.cursor = None;
    filter.limit = None;
    monitor_history::records::digest(&(endpoint, filter))
        .map(|s| s.as_ref().to_owned())
        .map_err(|_| ApiError::BadQuery)
}
pub fn resolve(
    filter: &mut Filter,
    endpoint: &str,
) -> Result<(Window, Option<(DateTime<Utc>, Id)>), ApiError> {
    filter.normalize().map_err(|_| ApiError::BadQuery)?;
    if let Some(cursor) = &filter.cursor {
        let cursor: Cursor = serde_json::from_str(cursor).map_err(|_| ApiError::BadQuery)?;
        if cursor.version != 1
            || cursor.fingerprint != fingerprint(filter, endpoint)?
            || cursor.window.from >= cursor.window.to
            || cursor.window.to - cursor.window.from > chrono::Duration::days(31)
            || cursor.at > cursor.window.to
        {
            return Err(ApiError::BadQuery);
        }
        Ok((cursor.window, Some((cursor.at, cursor.id))))
    } else {
        Ok((
            filter.window(Utc::now()).map_err(|_| ApiError::BadQuery)?,
            None,
        ))
    }
}
pub fn next(
    filter: &Filter,
    endpoint: &str,
    window: Window,
    record: &monitor_query::record::Record,
) -> Result<String, ApiError> {
    let id = Id::try_new(record.id.parse().map_err(|_| ApiError::BadQuery)?)
        .map_err(|_| ApiError::BadQuery)?;
    serde_json::to_string(&Cursor {
        version: 1,
        fingerprint: fingerprint(filter, endpoint)?,
        window,
        at: record.observed_at,
        id,
    })
    .map_err(|_| ApiError::BadQuery)
}

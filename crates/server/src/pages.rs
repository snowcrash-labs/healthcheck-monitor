//! Stable keyset cursors survive insertions and removals between page requests.
use crate::{
    response::ApiError,
    view::{FindingView, Resource},
};
use monitor_core::model::Severity;
use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    Next,
    Previous,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    priority: u8,
    resource: String,
    identity: String,
}
impl Cursor {
    fn key(&self) -> (u8, &str, &str) {
        (self.priority, &self.resource, &self.identity)
    }
}
pub trait Keyed {
    fn key(&self) -> (u8, &str, &str);
}
impl Keyed for Resource {
    fn key(&self) -> (u8, &str, &str) {
        (0, &self.id, "")
    }
}
impl Keyed for FindingView {
    fn key(&self) -> (u8, &str, &str) {
        let priority = match self.severity {
            Severity::Error => 0,
            Severity::Warning => 1,
            Severity::Info => 2,
        };
        (priority, &self.resource, &self.id)
    }
}
#[derive(Serialize)]
pub struct Page<T> {
    pub generation: u64,
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub previous_cursor: Option<String>,
    pub total: usize,
}
pub struct Slice<'a, T> {
    pub items: Vec<&'a T>,
    pub next: Option<String>,
    pub previous: Option<String>,
    pub total: usize,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopedCursor {
    scope: String,
    position: String,
}
/// Bind current-state cursors to their endpoint, filters, and ordering without claiming snapshot isolation.
pub fn select_scoped<'a, T: Keyed>(
    rows: Vec<&'a T>,
    after: Option<&str>,
    direction: &Direction,
    limit: usize,
    scope: &impl Serialize,
) -> Result<Slice<'a, T>, ApiError> {
    use sha2::{Digest, Sha256};
    let scope = serde_json::to_vec(scope).map_err(|_| ApiError::BadQuery)?;
    let scope: String = Sha256::digest(scope)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let cursor = match after {
        Some(value) if value.len() <= 32768 => {
            let cursor: ScopedCursor =
                serde_json::from_str(value).map_err(|_| ApiError::Refresh)?;
            if cursor.scope != scope {
                return Err(ApiError::Refresh);
            }
            Some(cursor.position)
        }
        Some(_) => return Err(ApiError::BadQuery),
        None => None,
    };
    let mut page = select(rows, cursor.as_deref(), direction, limit)?;
    let encode = |position: String| {
        serde_json::to_string(&ScopedCursor {
            scope: scope.clone(),
            position,
        })
        .map_err(|_| ApiError::BadQuery)
    };
    page.next = page.next.map(encode).transpose()?;
    page.previous = page.previous.map(encode).transpose()?;
    Ok(page)
}
fn cursor(row: &impl Keyed) -> Result<String, ApiError> {
    let (priority, resource, identity) = row.key();
    serde_json::to_string(&Cursor {
        priority,
        resource: resource.into(),
        identity: identity.into(),
    })
    .map_err(|_| ApiError::BadQuery)
}
pub fn select<'a, T: Keyed>(
    mut rows: Vec<&'a T>,
    after: Option<&str>,
    direction: &Direction,
    limit: usize,
) -> Result<Slice<'a, T>, ApiError> {
    rows.sort_unstable_by(|a, b| a.key().cmp(&b.key()));
    let total = rows.len();
    let boundary = match after {
        Some(value) if value.len() <= 16384 => {
            let value: Cursor = serde_json::from_str(value).map_err(|_| ApiError::BadQuery)?;
            if value.priority > 2 || value.resource.len() > 4096 || value.identity.len() > 8192 {
                return Err(ApiError::BadQuery);
            }
            Some(value)
        }
        Some(_) => return Err(ApiError::BadQuery),
        None => None,
    };
    let (start, end) = match (&boundary, direction) {
        (Some(boundary), Direction::Previous) => {
            let end = rows.partition_point(|row| row.key() < boundary.key());
            (end.saturating_sub(limit), end)
        }
        _ => {
            let start = boundary.as_ref().map_or(0, |boundary| {
                rows.partition_point(|row| row.key() <= boundary.key())
            });
            (start, start.saturating_add(limit).min(total))
        }
    };
    let items = rows[start..end].to_vec();
    let next = if end < total {
        items.last().map(|row| cursor(*row)).transpose()?
    } else {
        None
    };
    let previous = if start > 0 {
        items.first().map(|row| cursor(*row)).transpose()?
    } else {
        None
    };
    Ok(Slice {
        items,
        next,
        previous,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    impl Keyed for String {
        fn key(&self) -> (u8, &str, &str) {
            (0, self, "")
        }
    }
    #[test]
    fn insertion_and_cursor_removal_do_not_shift_the_next_page() -> Result<(), ApiError> {
        let mut rows: Vec<String> = ["b", "c", "d", "e"].into_iter().map(String::from).collect();
        let first = select(rows.iter().collect(), None, &Direction::Next, 2)?;
        let next = first.next.clone();
        drop(first);
        rows.remove(1);
        rows.insert(0, "a".into());
        let second = select(rows.iter().collect(), next.as_deref(), &Direction::Next, 2)?;
        assert_eq!(second.items, [&"d".to_string(), &"e".to_string()]);
        let previous = select(
            rows.iter().collect(),
            second.previous.as_deref(),
            &Direction::Previous,
            2,
        )?;
        assert_eq!(previous.items, [&"a".to_string(), &"b".to_string()]);
        assert!(select(rows.iter().collect(), Some("invalid"), &Direction::Next, 2).is_err());
        Ok(())
    }
}

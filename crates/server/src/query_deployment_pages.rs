//! Fold every selected release page while retaining only compact configured-check state.
use crate::{api::App, response::ApiError};
use monitor_query::{
    enums::Category,
    filter::{Deployment, Filter, Window},
    record::{Details, Record},
    release_assessment::ReleaseAssessment,
};
pub async fn checks(
    app: &App,
    filter: &Filter,
    window: Window,
    required: &[String],
) -> Result<Vec<Record>, ApiError> {
    let required: std::collections::BTreeSet<_> = required.iter().map(String::as_str).collect();
    let mut after = None;
    let mut retained = vec![];
    loop {
        let mut page = app
            .history
            .query_latest(filter, window, Category::Check, after.as_deref(), None)
            .await?;
        let more = page.len() > 100;
        page.truncate(100);
        after = page.last().map(|r| r.identity.clone());
        for mut record in page {
            if required.contains(record.identity.as_str()) {
                if let Details::Check { operations, .. } = &mut record.details {
                    operations.clear();
                }
                retained.push(record);
            }
        }
        if !more {
            return Ok(retained);
        }
    }
}
pub async fn releases(
    app: &App,
    query: &Deployment,
    window: Window,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<ReleaseAssessment, ApiError> {
    let mut status = ReleaseAssessment::default();
    let mut after = None;
    if query.expected_revision.is_none() && query.expected_digest.is_none() {
        return Ok(status);
    }
    loop {
        let mut page = app
            .history
            .query_latest(
                &query.filter,
                window,
                Category::Release,
                after.as_deref(),
                None,
            )
            .await?;
        let more = page.len() > 100;
        page.truncate(100);
        after = page.last().map(|r| r.identity.clone());
        for record in page {
            status.observe(query, &record, window, now);
        }
        if !more {
            return Ok(status);
        }
    }
}

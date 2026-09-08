//! Durable import accounting prevents restarts from resetting query allowances.
use crate::{
    History,
    cost_schema::{cost_import as i, cost_source as s},
    cost_write::Import,
    error::Error,
};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;

/// Unfinished jobs retain their reservation across midnight and configuration changes.
pub(crate) async fn reserve(
    connection: &mut diesel_async::AsyncPgConnection,
    additional: u64,
    allowance: u64,
) -> Result<(), Error> {
    let day = chrono::Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .ok_or(Error::Record)?
        .and_utc();
    let reservations: Vec<(i64, Option<i64>)> = i::table
        .filter(
            i::cost_import_started_at
                .ge(day)
                .or(i::cost_import_published_at.is_null()),
        )
        .select((i::cost_import_reserved_bytes, i::cost_import_billed_bytes))
        .limit(4097)
        .load(connection)
        .await?;
    let used = reservations
        .iter()
        .try_fold(0u64, |sum, (reserved, billed)| {
            sum.checked_add(billed.unwrap_or(*reserved) as u64)
                .ok_or(Error::Record)
        })?;
    if reservations.len() > 4096 || used.saturating_add(additional) > allowance {
        return Err(Error::Record);
    }
    Ok(())
}
impl History {
    pub async fn cost_settle(&self, import: &Import, bytes: u64) -> Result<(), Error> {
        let bytes = i64::try_from(bytes).map_err(|_| Error::Record)?;
        let mut c = self.cost_pool.get().await.map_err(|_| Error::Pool)?;
        diesel::update(i::table.find(import.id))
            .set(i::cost_import_billed_bytes.eq(bytes))
            .execute(&mut c)
            .await?;
        Ok(())
    }
    pub async fn cost_attempt(&self, import: &Import) -> Result<(), Error> {
        let mut c = self.cost_pool.get().await.map_err(|_| Error::Pool)?;
        diesel::update(i::table.find(import.id))
            .set(i::cost_import_attempted_at.eq(chrono::Utc::now()))
            .execute(&mut c)
            .await?;
        Ok(())
    }
    pub async fn cost_recent_attempt(&self, source: &str, seconds: i64) -> Result<bool, Error> {
        let mut c = self.cost_read_pool.get().await.map_err(|_| Error::Pool)?;
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(seconds);
        diesel::select(diesel::dsl::exists(
            i::table
                .inner_join(s::table.on(s::cost_source_id.eq(i::cost_import_source_id)))
                .filter(s::cost_source_name.eq(source))
                .filter(i::cost_import_attempted_at.ge(cutoff)),
        ))
        .get_result(&mut c)
        .await
        .map_err(Error::from)
    }
}

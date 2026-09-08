//! Durable import accounting prevents restarts from resetting query allowances.
use crate::{
    History,
    cost_schema::{cost_import as i, cost_source as s},
    cost_write::Import,
    error::Error,
};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
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

//! Query records use batched idempotent upserts and bounded retention passes.
use crate::{
    error::Error,
    pool::History,
    query_records::{NewRecord, QueryRecord},
    query_schema::{query_record as r, query_watermark as w},
    types::Id,
};
use diesel::{prelude::*, upsert::excluded};
use diesel_async::RunQueryDsl;

pub(crate) async fn write(
    connection: &mut diesel_async::AsyncPgConnection,
    records: &[QueryRecord],
) -> Result<(), Error> {
    let unique: std::collections::BTreeMap<&str, &QueryRecord> =
        records.iter().map(|r| (r.key.as_ref(), r)).collect();
    let unique: Vec<_> = unique.into_values().collect();
    for batch in unique.chunks(128) {
        let rows: Vec<_> = batch
            .iter()
            .map(|r| NewRecord::try_from(*r))
            .collect::<Result<_, _>>()?;
        diesel::insert_into(r::table)
            .values(&rows)
            .on_conflict(r::query_record_key)
            .do_update()
            .set((
                r::query_record_to.eq(excluded(r::query_record_to)),
                r::query_record_closed_at.eq(excluded(r::query_record_closed_at)),
                r::query_record_state.eq(excluded(r::query_record_state)),
                r::query_record_payload.eq(excluded(r::query_record_payload)),
                r::query_record_written_at.eq(diesel::dsl::now),
            ))
            .execute(connection)
            .await?;
    }
    if !records.is_empty() {
        diesel::update(w::table)
            .set(w::query_watermark_persisted_at.eq(diesel::dsl::now))
            .execute(connection)
            .await?;
    }
    Ok(())
}
impl History {
    pub(crate) async fn retain_queries(&self) -> Result<(), Error> {
        let mut connection = self.pool.get().await.map_err(|_| Error::Pool)?;
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(self.config.retention.0 as i64);
        let expired: Vec<Id> = r::table
            .filter(r::query_record_to.lt(cutoff))
            .order((r::query_record_to.asc(), r::query_record_id.asc()))
            .limit(2048)
            .select(r::query_record_id)
            .load(&mut connection)
            .await?;
        if !expired.is_empty() {
            diesel::delete(r::table.filter(r::query_record_id.eq_any(expired)))
                .execute(&mut connection)
                .await?;
        }
        let count: i64 = r::table.count().get_result(&mut connection).await?;
        if count > self.config.diagnostic_rows {
            let oldest: Vec<(Id, chrono::DateTime<chrono::Utc>)> = r::table
                .order((r::query_record_to.asc(), r::query_record_id.asc()))
                .limit((count - self.config.diagnostic_rows).min(2048))
                .select((r::query_record_id, r::query_record_to))
                .load(&mut connection)
                .await?;
            if let Some((_, at)) = oldest.last() {
                diesel::update(w::table)
                    .set(w::query_watermark_evicted_through.eq(Some(*at)))
                    .execute(&mut connection)
                    .await?;
                diesel::delete(
                    r::table.filter(
                        r::query_record_id
                            .eq_any(oldest.into_iter().map(|(id, _)| id).collect::<Vec<_>>()),
                    ),
                )
                .execute(&mut connection)
                .await?;
            }
        }
        Ok(())
    }
}

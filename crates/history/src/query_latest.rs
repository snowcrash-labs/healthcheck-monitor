//! Latest evidence per source is selected in PostgreSQL, without scanning all historical pages.
use crate::{
    enums, error::Error, pool::History, query_records::Stored, query_schema::query_record as r,
};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use monitor_query::{
    enums::Category,
    filter::{Filter, Window},
    record::Record,
};
impl History {
    pub async fn query_latest(
        &self,
        filter: &Filter,
        window: Window,
        category: Category,
        after: Option<&str>,
        identities: Option<&[String]>,
    ) -> Result<Vec<Record>, Error> {
        if !self.ready() {
            return Err(Error::Migration);
        }
        let mut selection = crate::query_read::selection(filter, window, Some(category));
        if let Some(after) = after {
            selection = selection.filter(r::query_record_identity.gt(after.to_owned()));
        }
        if let Some(identities) = identities {
            selection = selection.filter(r::query_record_identity.eq_any(identities.to_vec()));
        }
        let selected = selection.select(r::query_record_id);
        let query = r::table
            .filter(r::query_record_id.eq_any(selected))
            .filter(r::query_record_category.eq(enums::QueryCategory::from(category)));
        let mut connection = self.read_pool.get().await.map_err(|_| Error::Pool)?;
        let rows: Vec<Stored> = query
            .distinct_on(r::query_record_identity)
            .order((
                r::query_record_identity.asc(),
                r::query_record_from.desc(),
                r::query_record_id.desc(),
            ))
            .limit(101)
            .select(Stored::as_select())
            .load(&mut connection)
            .await?;
        rows.into_iter().map(Stored::decode).collect()
    }
}

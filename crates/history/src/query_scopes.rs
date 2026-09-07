//! Scope discovery includes retained targets after they disappear from current configuration.
use crate::{
    enums,
    error::Error,
    pool::History,
    query_schema::query_record as r,
    types::{Name, Resource},
};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use monitor_query::{
    filter::{Filter, Window},
    record::Scope,
};
impl History {
    pub async fn query_scopes(
        &self,
        filter: &Filter,
        window: Window,
        after: Option<&Scope>,
    ) -> Result<Vec<Scope>, Error> {
        if !self.ready() {
            return Err(Error::Migration);
        }
        let mut query = crate::query_read::selection(filter, window, None);
        if let Some(after) = after {
            let provider = enums::QueryProvider::from(after.provider);
            query = query.filter(
                r::query_record_target
                    .gt(after.target.clone())
                    .or(r::query_record_target
                        .eq(after.target.clone())
                        .and(r::query_record_provider.gt(provider)))
                    .or(r::query_record_target
                        .eq(after.target.clone())
                        .and(r::query_record_provider.eq(provider))
                        .and(r::query_record_scope.gt(after.scope.clone()))),
            );
        }
        let mut connection = self.read_pool.get().await.map_err(|_| Error::Pool)?;
        let rows: Vec<(Name, enums::QueryProvider, Resource)> = query
            .select((
                r::query_record_target,
                r::query_record_provider,
                r::query_record_scope,
            ))
            .distinct()
            .order((
                r::query_record_target.asc(),
                r::query_record_provider.asc(),
                r::query_record_scope.asc(),
            ))
            .limit(i64::from(filter.limit.unwrap_or(50)) + 1)
            .load(&mut connection)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(target, provider, scope)| Scope {
                target: target.as_ref().to_owned(),
                provider: provider.into(),
                scope: scope.as_ref().to_owned(),
            })
            .collect())
    }
}

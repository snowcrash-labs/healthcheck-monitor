//! Missing scheduled observations remain explicit even after the process or database recovers.
use crate::{enums, error::Error, pool::History};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use monitor_query::filter::{Filter, Window};
diesel::table! {
    use diesel::sql_types::*;
    use crate::query_schema::sql_types::QueryProvider;
    use crate::schema::sql_types::CheckKind;
    health_monitor.query_gap (query_gap_id) {
        query_gap_id -> Uuid,
        query_gap_target -> Varchar,
        query_gap_provider -> QueryProvider,
        query_gap_scope -> Varchar,
        query_gap_check -> Nullable<CheckKind>,
        query_gap_from -> Timestamptz,
        query_gap_to -> Timestamptz,
    }
}
impl History {
    pub async fn query_gap_count(&self, filter: &Filter, window: Window) -> Result<i64, Error> {
        if !self.ready() {
            return Err(Error::Migration);
        }
        use query_gap as g;
        let mut query = g::table
            .into_boxed()
            .filter(g::query_gap_from.lt(window.to))
            .filter(g::query_gap_to.gt(window.from));
        if let Some(target) = &filter.target {
            query = query.filter(g::query_gap_target.eq(target.clone()));
        }
        if let Some(scope) = &filter.scope {
            query = query.filter(g::query_gap_scope.eq(scope.clone()));
        }
        if let Some(provider) = filter.provider {
            query = query.filter(g::query_gap_provider.eq(enums::QueryProvider::from(provider)));
        }
        if let Some(check) = filter.check {
            query = query.filter(g::query_gap_check.eq(enums::Check::from(check)));
        }
        let mut connection = self.read_pool.get().await.map_err(|_| Error::Pool)?;
        query
            .count()
            .get_result(&mut connection)
            .await
            .map_err(Error::from)
    }
}

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
        let query = query
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
            .limit(i64::from(filter.limit.unwrap_or(50)) + 1);
        let rows: Vec<(Name, enums::QueryProvider, Resource)> = connection
            .build_transaction()
            .read_only()
            .run(async |connection| {
                // Correlated scope columns make incremental-sort LIMIT plans scan nearly every row.
                diesel::select(scope_setting("enable_incremental_sort", "off", true))
                    .get_result::<String>(connection)
                    .await?;
                query.load(connection).await.map_err(Error::from)
            })
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

diesel::define_sql_function! {
    #[sql_name = "set_config"]
    fn scope_setting(name: diesel::sql_types::Text, value: diesel::sql_types::Text, local: diesel::sql_types::Bool) -> diesel::sql_types::Text;
}

#[cfg(test)]
mod tests {
    use super::*;
    diesel::define_sql_function! {
        fn current_setting(name: diesel::sql_types::Text) -> diesel::sql_types::Text;
    }

    #[tokio::test]
    #[ignore = "requires isolated HEALTHCHECK_TEST_DATABASE_URL"]
    async fn scope_planner_setting_is_local_on_success_and_failure()
    -> Result<(), Box<dyn std::error::Error>> {
        let url = std::env::var("HEALTHCHECK_TEST_DATABASE_URL")?;
        let parsed: tokio_postgres::Config = url.parse()?;
        if parsed.get_dbname() != Some("healthcheck_monitor_dashboard_test") {
            return Err("refusing non-test database".into());
        }
        let history = History::new(url, Default::default())?;
        history.migrate().await?;
        let mut connection = history.read_pool.get().await?;
        diesel::select(scope_setting("enable_incremental_sort", "on", false))
            .get_result::<String>(&mut connection)
            .await?;
        drop(connection);
        let now = chrono::Utc::now();
        let window = Window {
            from: now - chrono::Duration::minutes(1),
            to: now,
        };
        history
            .query_scopes(&Filter::default(), window, None)
            .await?;
        let mut connection = history.read_pool.get().await?;
        let setting: String = diesel::select(current_setting("enable_incremental_sort"))
            .get_result(&mut connection)
            .await?;
        assert_eq!(setting, "on");
        drop(connection);
        let invalid = Filter {
            scope: Some("invalid\0scope".into()),
            ..Default::default()
        };
        assert!(history.query_scopes(&invalid, window, None).await.is_err());
        let mut connection = history.read_pool.get().await?;
        let setting: String = diesel::select(current_setting("enable_incremental_sort"))
            .get_result(&mut connection)
            .await?;
        assert_eq!(setting, "on");
        let (started, ready) = tokio::sync::oneshot::channel();
        {
            let transaction = connection
                .build_transaction()
                .read_only()
                .run(async |connection| {
                    diesel::select(scope_setting("enable_incremental_sort", "off", true))
                        .get_result::<String>(connection)
                        .await?;
                    started.send(()).map_err(|_| Error::Task)?;
                    std::future::pending::<Result<(), Error>>().await
                });
            tokio::pin!(transaction);
            tokio::select! {
                _ = &mut transaction => return Err("transaction unexpectedly completed".into()),
                result = tokio::time::timeout(std::time::Duration::from_secs(5), ready) => { result??; }
            }
        }
        drop(connection);
        let mut connection = history.read_pool.get().await?;
        let setting: String = diesel::select(current_setting("enable_incremental_sort"))
            .get_result(&mut connection)
            .await?;
        assert_eq!(setting, "on");
        Ok(())
    }
}

//! PostgreSQL 18 billing contracts run against an explicitly isolated local test database.
use monitor_costs::{
    config::{Config, Gcp, ReaderAccess, Source},
    model::{Charge, Provider},
    query::{Filter, Group, Period},
};
use monitor_history::History;
#[path = "costs/mod.rs"]
mod billing;

fn charge(
    day: chrono::NaiveDate,
    product: &str,
    amount: &str,
) -> Result<Charge, monitor_costs::error::Error> {
    Ok(Charge {
        target: None,
        day,
        invoice_month: Some("202609".into()),
        provider: Provider::Gcp,
        scope: Some("example".into()),
        region: Some("us-central1".into()),
        product: product.into(),
        resource: None,
        category: Some("usage".into()),
        currency: "USD".into(),
        billed: amount.to_owned().try_into()?,
        effective: None,
    })
}
#[tokio::test]
#[ignore = "requires HEALTHCHECK_COST_TEST_DATABASE_URL for an isolated PostgreSQL 18 database"]
async fn corrections_partial_imports_cursors_and_signed_totals()
-> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("HEALTHCHECK_COST_TEST_DATABASE_URL")?;
    let parsed: tokio_postgres::Config = url.parse()?;
    if parsed.get_dbname() != Some("healthcheck_monitor_costs_test") {
        return Err("refusing non-test database".into());
    }
    let history = History::new(url, Default::default())?;
    history.migrate().await?;
    let source = Source {
        id: "synthetic".into(),
        aws_query: None,
        azure_query: None,
        credential: None,
        provider: Provider::Gcp,
        billing_scope: "test-account".into(),
        gcp: Some(Gcp {
            project: "example".into(),
            dataset: "billing".into(),
            table: "export".into(),
            location: "US".into(),
            detailed: true,
        }),
    };
    let config = Config {
        enabled: true,
        reader_access: Some(ReaderAccess::DashboardReaders),
        sources: vec![source.clone()],
        max_bytes_billed: Some(1),
        daily_bytes_billed: Some(1000),
        ..Default::default()
    };
    config.validate()?;
    let period = Period {
        from: chrono::Utc::now().date_naive() - chrono::Duration::days(1),
        to: chrono::Utc::now().date_naive(),
    };
    let filter = Filter {
        from: Some(period.from),
        to: Some(period.to),
        group: Group::Product,
        limit: Some(1),
        ..Default::default()
    };
    let first = history.cost_begin(&source, period, &config).await?;
    assert_eq!(first.id.as_ref().get_version_num(), 7);
    history
        .cost_stage(
            &first,
            vec![
                charge(period.from, "Compute", "0.3")?,
                charge(period.from, "Storage", "-0.1")?,
            ],
        )
        .await?;
    history.cost_publish(&first, 2, config.retention()).await?;
    let before = history.cost_view(&filter, &config).await?;
    assert_eq!(before.total.map(String::from).as_deref(), Some("0.2"));
    assert_eq!(before.contributor_count, 2);
    assert_eq!(before.breakdown.len(), 1);
    let cursor = before.next_cursor.ok_or("next cursor")?;
    let second_page = history
        .cost_view(
            &Filter {
                cursor: Some(cursor.clone()),
                revision: Some(before.revision.clone()),
                ..filter.clone()
            },
            &config,
        )
        .await?;
    assert_eq!(second_page.breakdown[0].key, "v:Storage");
    assert_eq!(second_page.total.map(String::from).as_deref(), Some("0.2"));
    assert!(
        history
            .cost_view(
                &Filter {
                    cursor: Some(cursor.clone()),
                    currency: Some("EUR".into()),
                    ..filter.clone()
                },
                &config
            )
            .await
            .is_err()
    );
    let replacement = history.cost_begin(&source, period, &config).await?;
    history
        .cost_stage(&replacement, vec![charge(period.from, "Compute", "1.1")?])
        .await?;
    assert_eq!(
        history
            .cost_view(&filter, &config)
            .await?
            .total
            .map(String::from)
            .as_deref(),
        Some("0.2")
    );
    assert!(
        history
            .cost_publish(&replacement, 2, config.retention())
            .await
            .is_err()
    );
    let resumed = history.cost_begin(&source, period, &config).await?;
    assert_eq!(replacement.id, resumed.id);
    history
        .cost_stage(
            &resumed,
            vec![
                charge(period.from, "Compute", "1.2")?,
                charge(period.from, "Storage", "-0.2")?,
            ],
        )
        .await?;
    history
        .cost_publish(&resumed, 2, config.retention())
        .await?;
    assert_eq!(
        history
            .cost_view(&filter, &config)
            .await?
            .total
            .map(String::from)
            .as_deref(),
        Some("1")
    );
    assert!(
        history
            .cost_view(
                &Filter {
                    cursor: Some(cursor),
                    revision: Some(before.revision),
                    ..filter
                },
                &config
            )
            .await
            .is_err()
    );
    let providers = history
        .cost_view(
            &Filter {
                from: Some(period.from),
                to: Some(period.to),
                ..Default::default()
            },
            &config,
        )
        .await?;
    assert_eq!(providers.breakdown[0].key, "v:gcp");
    let unallocated = history
        .cost_view(
            &Filter {
                from: Some(period.from),
                to: Some(period.to),
                group: Group::Resource,
                ..Default::default()
            },
            &config,
        )
        .await?;
    assert_eq!(unallocated.contributor_count, 1);
    assert_eq!(unallocated.breakdown[0].key, "v:");
    let scheduled = history
        .cost_next_period(&source, &config)
        .await?
        .ok_or("backfill period")?;
    assert!((scheduled.to - scheduled.from).num_days() <= 7);
    let aws_source = Source {
        id: "synthetic-aws".into(),
        provider: Provider::Aws,
        billing_scope: "123456789012".into(),
        gcp: None,
        aws_query: Some(monitor_costs::config::AwsQuery {
            region: "us-east-1".into(),
        }),
        ..source.clone()
    };
    let aws_window = history
        .cost_next_period(&aws_source, &config)
        .await?
        .ok_or("AWS backfill window")?;
    assert_eq!(
        (aws_window.to - aws_window.from).num_days(),
        i64::from(config.backfill())
    );
    billing::drilldown::verify(&history, &source, &config, period.to).await?;
    billing::reservation::verify(&history, &source, &config, period).await?;
    history.cost_cleanup().await?;
    Ok(())
}

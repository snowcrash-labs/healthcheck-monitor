//! Remainder drilldowns retain the parent range and page complete contributor comparisons.
use monitor_costs::{
    config::{Config, Source},
    query::{Filter, Group, Period},
};
use monitor_history::History;

pub(crate) async fn verify(
    history: &History,
    source: &Source,
    config: &Config,
    to: chrono::NaiveDate,
) -> Result<(), Box<dyn std::error::Error>> {
    let from = to - chrono::Duration::days(2);
    let period = Period {
        from: from - chrono::Duration::days(2),
        to,
    };
    let import = history.cost_begin(source, period, config).await?;
    let mut charges = Vec::new();
    for offset in 0..4 {
        let day = period.from + chrono::Duration::days(offset);
        for index in 0..10 {
            let amount = (100 - index * 10) / if day < from { 2 } else { 1 };
            charges.push(crate::charge(
                day,
                &format!("P{index:02}"),
                &amount.to_string(),
            )?);
        }
    }
    history.cost_stage(&import, charges).await?;
    history
        .cost_publish(&import, 40, config.retention())
        .await?;
    let filter = Filter {
        from: Some(from),
        to: Some(to),
        group: Group::Product,
        limit: Some(2),
        ..Default::default()
    };
    let all = history.cost_view(&filter, config).await?;
    assert!(
        all.series
            .iter()
            .all(|point| point.contributors.iter().any(|row| row.key == "__other__"))
    );
    let remainder = Filter {
        contributor: Some("__other__".into()),
        ..filter
    };
    let view = history.cost_view(&remainder, config).await?;
    assert_eq!(view.contributor_count, 3);
    assert_eq!(view.total.map(String::from).as_deref(), Some("120"));
    assert_eq!(view.breakdown[0].key, "v:P07");
    assert_eq!(
        view.breakdown[0]
            .previous
            .clone()
            .map(String::from)
            .as_deref(),
        Some("30")
    );
    let next = history
        .cost_view(
            &Filter {
                cursor: view.next_cursor,
                ..remainder.clone()
            },
            config,
        )
        .await?;
    assert_eq!(next.breakdown[0].key, "v:P09");
    let searched = history
        .cost_view(
            &Filter {
                q: Some("P08".into()),
                ..remainder.clone()
            },
            config,
        )
        .await?;
    assert_eq!(searched.contributor_count, 1);
    assert_eq!(searched.total.map(String::from).as_deref(), Some("40"));
    let day = history
        .cost_view(
            &Filter {
                day: Some(from),
                ..remainder.clone()
            },
            config,
        )
        .await?;
    assert_eq!(day.contributor_count, 3);
    assert_eq!(day.total.map(String::from).as_deref(), Some("60"));
    assert!(
        history
            .cost_view(
                &Filter {
                    from: Some(from - chrono::Duration::days(1)),
                    day: Some(from),
                    cursor: day.next_cursor,
                    ..remainder.clone()
                },
                config
            )
            .await
            .is_err()
    );
    let incomplete = history
        .cost_view(
            &Filter {
                from: Some(from - chrono::Duration::days(10)),
                ..remainder
            },
            config,
        )
        .await?;
    assert!(incomplete.previous_total.is_none());
    assert!(
        incomplete
            .breakdown
            .iter()
            .all(|row| row.previous.is_none())
    );
    Ok(())
}

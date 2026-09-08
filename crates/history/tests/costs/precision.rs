//! Database publication preserves the smallest supported exact charge through a read cycle.
use monitor_costs::{
    config::{Config, Source},
    query::{Filter, Period},
};
use monitor_history::History;

pub(crate) async fn verify(
    history: &History,
    source: &Source,
    config: &Config,
    period: Period,
) -> Result<(), Box<dyn std::error::Error>> {
    let tiny = "0.00000000000000000000000000000000000001";
    let import = history.cost_begin(source, period, config).await?;
    history
        .cost_stage(
            &import,
            vec![crate::charge(period.from, "Fractional usage", tiny)?],
        )
        .await?;
    history.cost_publish(&import, 1, config.retention()).await?;
    let view = history
        .cost_view(
            &Filter {
                from: Some(period.from),
                to: Some(period.to),
                product: Some("Fractional usage".into()),
                ..Default::default()
            },
            config,
        )
        .await?;
    assert_eq!(view.total.map(String::from).as_deref(), Some(tiny));
    Ok(())
}

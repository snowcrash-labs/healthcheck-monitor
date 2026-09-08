//! Raising a pending query allowance reserves the increase before another query can spend it.
use monitor_costs::{
    config::{Config, Source},
    query::Period,
};
use monitor_history::History;

pub(crate) async fn verify(
    history: &History,
    source: &Source,
    config: &Config,
    period: Period,
) -> Result<(), Box<dyn std::error::Error>> {
    let source = Source {
        id: "reservation".into(),
        billing_scope: "reservation-account".into(),
        ..source.clone()
    };
    let initial = history.cost_begin(&source, period, config).await?;
    let excessive = Config {
        max_bytes_billed: Some(1000),
        ..config.clone()
    };
    assert!(
        history
            .cost_begin(&source, period, &excessive)
            .await
            .is_err()
    );
    let increased = Config {
        max_bytes_billed: Some(500),
        ..config.clone()
    };
    let resumed = history.cost_begin(&source, period, &increased).await?;
    assert_eq!(initial.id, resumed.id);
    let competing = Source {
        id: "competing-reservation".into(),
        billing_scope: "competing-reservation-account".into(),
        ..source.clone()
    };
    let competing_config = Config {
        max_bytes_billed: Some(600),
        ..config.clone()
    };
    assert!(
        history
            .cost_begin(&competing, period, &competing_config)
            .await
            .is_err()
    );
    assert_eq!(
        history.cost_begin(&source, period, config).await?.id,
        initial.id
    );
    Ok(())
}

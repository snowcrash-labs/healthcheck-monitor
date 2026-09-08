//! Synthetic billing regressions include credits, precision, and incomplete comparisons.
use crate::{
    aggregate::{self, Bucket},
    config::{Config, ReaderAccess},
    model::{Amount, Charge, Provider},
    query::{Filter, Granularity, Period},
};
#[test]
fn amounts_reject_lossy_and_oversized_values() -> Result<(), Box<dyn std::error::Error>> {
    let a = Amount::try_from("0.1".to_owned())?;
    let b = Amount::try_from("0.2".to_owned())?;
    assert_eq!(String::from(a.add(&b)?), "0.3");
    for bad in [
        "NaN",
        "1e20",
        "1.000000000000000000000000000000000000001",
        "100000000000000000000",
        " 1",
        "+1",
    ] {
        assert!(Amount::try_from(bad.to_owned()).is_err(), "{bad}");
    }
    assert_eq!(
        String::from(Amount::provider("1e-38")?),
        "0.00000000000000000000000000000000000001"
    );
    Ok(())
}
#[test]
fn credit_is_applied_once_and_prior_is_not_added() -> Result<(), Box<dyn std::error::Error>> {
    let from = "2026-08-01".parse()?;
    let to = "2026-08-03".parse()?;
    let rows = vec![
        Bucket {
            day: from,
            key: "gcp".into(),
            amount: "10.25".to_string().try_into()?,
        },
        Bucket {
            day: from,
            key: "gcp".into(),
            amount: "-1.5".to_string().try_into()?,
        },
        Bucket {
            day: "2026-07-30".parse()?,
            key: "gcp".into(),
            amount: "5".to_string().try_into()?,
        },
    ];
    let period = Period { from, to };
    let series = aggregate::series(&rows, period, Granularity::Daily, true)?;
    assert_eq!(String::from(aggregate::total(&rows, period)?), "8.75");
    assert_eq!(String::from(series[0].total.clone()), "8.75");
    assert_eq!(
        series[0].previous.as_ref().map(|v| String::from(v.clone())),
        Some("5".into())
    );
    assert!(
        aggregate::series(&rows, period, Granularity::Daily, false)?[0]
            .previous
            .is_none()
    );
    Ok(())
}
#[test]
fn configuration_requires_explicit_readership_and_finite_allowances() {
    let mut config = Config {
        enabled: true,
        ..Default::default()
    };
    assert!(config.validate().is_err());
    config.reader_access = Some(ReaderAccess::DashboardReaders);
    assert!(config.validate().is_ok());
    config.daily_bytes_billed = Some(1);
    assert!(config.validate().is_err());
}
#[test]
fn charge_and_range_validation_agree_with_storage_constraints()
-> Result<(), Box<dyn std::error::Error>> {
    let mut row = Charge {
        target: None,
        day: "2026-08-01".parse()?,
        invoice_month: Some("202608".into()),
        provider: Provider::Gcp,
        scope: Some("example".into()),
        region: None,
        product: "Compute".into(),
        resource: None,
        category: Some("usage".into()),
        currency: "USD".into(),
        billed: Amount::zero(),
        effective: None,
    };
    row.validate()?;
    row.invoice_month = Some("202613".into());
    assert!(row.validate().is_err());
    let invalid = Filter {
        from: Some("2020-01-01".parse()?),
        to: Some("2026-01-01".parse()?),
        ..Default::default()
    };
    assert!(invalid.period().is_err());
    Ok(())
}

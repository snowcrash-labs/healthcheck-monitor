//! SLO compliance and urgent burn policy require valid, configured objectives.
use chrono::Utc;
use monitor_core::{
    config::{resolve::Selection, settings::Settings, types::Config},
    model::*,
    policy::evaluate,
};
fn slo(compliance: Option<f64>, burn_rate: Option<f64>) -> Observation {
    Observation {
        resource: "test/slo/api".into(),
        operation: "slo".into(),
        observed_at: Utc::now(),
        expected: Expected::Active,
        data: Data::Slo {
            goal: 0.999,
            compliance,
            budget: None,
            burn_rate,
            period_seconds: Some(86400),
        },
    }
}
#[test]
fn compliance_failure_is_distinct_from_an_urgent_configured_burn() {
    let obs = slo(Some(0.98), Some(20.0));
    assert_eq!(
        evaluate(&obs, None, &Settings::default(), Utc::now()).health,
        Health::Degraded
    );
    let settings = Settings {
        slo_burn_rate_error: Some(10.0),
        ..Default::default()
    };
    let evaluation = evaluate(&obs, None, &settings, Utc::now());
    assert_eq!(evaluation.health, Health::Unhealthy);
    assert_eq!(evaluation.findings[0].rule, "slo-burn-rate");
}
#[test]
fn missing_or_invalid_compliance_is_unknown() {
    for value in [None, Some(1.1), Some(f64::NAN)] {
        assert_eq!(
            evaluate(&slo(value, None), None, &Settings::default(), Utc::now()).health,
            Health::Unknown
        );
    }
}
#[test]
fn azure_slos_require_explicit_compliance_metrics_and_fraction_goals()
-> Result<(), Box<dyn std::error::Error>> {
    let text = "version=1\n[[targets]]\nname='test'\nprovider='azure'\nscope='subscription'\n[targets.slo_goals]\navailability=0.999\n[[targets.metrics]]\nname='availability'\nnamespace='custom'\nmetric='availability'\nresource='/subscriptions/subscription/resourceGroups/group/providers/Microsoft.Web/sites/api'\naggregation='latest'";
    let config = Config::parse(text)?;
    assert!(
        config
            .resolve(&Selection::default())?
            .jobs
            .iter()
            .any(|job| job.check == Check::Slo)
    );
    assert!(Config::parse(&text.replace("aggregation='latest'", "aggregation='minimum'")).is_err());
    assert!(Config::parse(&text.replace("availability=0.999", "availability=99.9")).is_err());
    Ok(())
}

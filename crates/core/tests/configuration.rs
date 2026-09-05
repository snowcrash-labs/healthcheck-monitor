//! Resolution, bounded settings, and selection contracts.
use monitor_core::{
    config::{duration::Span, resolve::Selection, settings::SettingsPatch, types::Config},
    model::Check,
};
fn config(extra: &str) -> Result<Config, monitor_core::error::Error> {
    Config::parse(&format!(
        "version = 1\n{extra}\n[[targets]]\nname = \"dev\"\nprovider = \"gcp\"\nscope = \"dev-project\"\nregions = [\"us-central1\"]\n"
    ))
}
#[test]
fn default_profiles_are_editable() -> Result<(), Box<dyn std::error::Error>> {
    let c = config("")?;
    let quick = c.resolve(&Selection {
        profile: Some("quick".into()),
        ..Default::default()
    })?;
    assert!(quick.jobs.iter().all(|j| j.settings.samples == 1));
    let deep = c.resolve(&Selection {
        profile: Some("deep".into()),
        ..Default::default()
    })?;
    assert!(
        deep.jobs
            .iter()
            .all(|j| j.settings.log_entries == 5000 && j.settings.log_window == Span(86400))
    );
    Ok(())
}
#[test]
fn cli_overrides_global_profile_target_and_check() -> Result<(), Box<dyn std::error::Error>> {
    let c = Config::parse(
        "version=1\n[settings]\ninterval='40s'\n[profiles.full.settings]\ninterval='50s'\n[checks.queues]\ninterval='70s'\n[[targets]]\nname='dev'\nprovider='gcp'\nscope='dev-project'\n[targets.settings]\ninterval='60s'\n[targets.checks.queues]\ninterval='80s'",
    )?;
    let selected = Selection {
        checks: vec![Check::Queues],
        overrides: SettingsPatch {
            interval: Some(Span(90)),
            ..Default::default()
        },
        ..Default::default()
    };
    let jobs = c.resolve(&selected)?.jobs;
    assert!(jobs.iter().all(|j| j.settings.interval == Span(90)));
    assert!(jobs.iter().any(|j| j.check == Check::Inventory));
    Ok(())
}
#[test]
fn single_sample_does_not_change_other_limits() -> Result<(), Box<dyn std::error::Error>> {
    let c = config("")?;
    let effective = c.resolve(&Selection {
        checks: vec![Check::Queues],
        overrides: SettingsPatch {
            samples: Some(1),
            ..Default::default()
        },
        ..Default::default()
    })?;
    assert!(effective.jobs.iter().all(|j| j.settings.samples == 1));
    Ok(())
}
#[test]
fn duplicate_target_names_are_rejected() {
    let c = Config::parse(
        "version=1\n[[targets]]\nname='a'\nprovider='edge'\nscope='x'\n[[targets]]\nname='a'\nprovider='edge'\nscope='y'",
    );
    assert!(c.is_err());
}
#[test]
fn endpoint_requires_https_and_explicit_statuses() {
    for (url, statuses) in [
        ("http://example.com", "[200]"),
        ("https://example.com", "[]"),
        ("https://user:password@example.com", "[200]"),
    ] {
        let c = Config::parse(&format!(
            "version=1\n[[targets]]\nname='x'\nprovider='edge'\nscope='x'\n[[targets.endpoints]]\nname='x'\nurl='{url}'\naccepted={statuses}"
        ));
        assert!(c.is_err());
    }
}
#[test]
fn unknown_config_and_version_fail_closed() {
    assert!(config("typo=2").is_err());
    assert!(Config::parse("version=2\ntargets=[]").is_err());
}
#[test]
fn excessive_limits_rejected_before_scheduling() -> Result<(), Box<dyn std::error::Error>> {
    for body in [
        "concurrency=0",
        "scope_concurrency=50",
        "samples=0",
        "max_pages=0",
        "response_bytes=1000000000",
        "attempt_timeout='100s'",
        "history_count=0",
        "capacity_warning=95.0",
    ] {
        let c = config(&format!("[settings]\n{body}"))?;
        assert!(c.resolve(&Selection::default()).is_err());
    }
    Ok(())
}
#[test]
fn disabled_checks_are_not_scheduled() -> Result<(), Box<dyn std::error::Error>> {
    let c = config("[checks.logs]\nenabled=false")?;
    assert!(
        !c.resolve(&Selection::default())?
            .jobs
            .iter()
            .any(|j| j.check == Check::Logs)
    );
    Ok(())
}
#[test]
fn unknown_target_is_an_error() -> Result<(), Box<dyn std::error::Error>> {
    assert!(
        config("")?
            .resolve(&Selection {
                targets: vec!["missing".into()],
                ..Default::default()
            })
            .is_err()
    );
    Ok(())
}
#[test]
fn invalid_reload_does_not_mutate_original_config() -> Result<(), Box<dyn std::error::Error>> {
    let old = config("")?.resolve(&Selection::default())?;
    assert!(Config::parse("invalid!").is_err());
    let new = config("")?.resolve(&Selection::default())?;
    assert_eq!(old.revision, new.revision);
    Ok(())
}

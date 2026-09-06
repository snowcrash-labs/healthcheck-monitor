//! Cron calendars, timezones, and owner identity cannot be inferred from names alone.
use chrono::{DateTime, Duration, Utc};
use monitor_core::{
    config::settings::Settings, model::*, policy::evaluate, schedule_policy::correlate,
};
fn at(text: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    Ok(DateTime::parse_from_rfc3339(text)?.to_utc())
}
fn observation(data: Data, now: DateTime<Utc>) -> Observation {
    Observation {
        context: None,
        resource: "dev/cronjobs/ns/nightly".into(),
        operation: "cronjobs".into(),
        observed_at: now,
        expected: Expected::Active,
        data,
    }
}
fn schedule(now: DateTime<Utc>, zone: &str) -> Data {
    Data::Schedule {
        created_at: Some(now - Duration::days(3)),
        starting_deadline_seconds: None,
        forbid_overlap: false,
        schedule: "0 9 * * *".into(),
        timezone: zone.into(),
        suspended: false,
        active: 0,
        last_schedule: Some(now - Duration::days(1)),
        last_success: Some(now - Duration::days(1) + Duration::minutes(1)),
    }
}
#[test]
fn missed_run_uses_the_configured_timezone() -> Result<(), Box<dyn std::error::Error>> {
    let now = at("2026-09-05T10:00:00Z")?;
    let mut local = observation(schedule(now, "America/Denver"), now);
    if let Data::Schedule {
        last_schedule,
        last_success,
        ..
    } = &mut local.data
    {
        *last_schedule = Some(at("2026-09-04T15:00:00Z")?);
        *last_success = Some(at("2026-09-04T15:01:00Z")?);
    }
    let utc = observation(schedule(now, "UTC"), now);
    assert_eq!(
        evaluate(&local, None, &Settings::default(), now).health,
        Health::Healthy
    );
    assert_eq!(
        evaluate(&utc, None, &Settings::default(), now).health,
        Health::Degraded
    );
    Ok(())
}
#[test]
fn grace_new_controllers_and_unknown_timezones_do_not_fabricate_misses()
-> Result<(), Box<dyn std::error::Error>> {
    let now = at("2026-09-05T09:05:00Z")?;
    let obs = observation(schedule(now, "UTC"), now);
    assert_ne!(
        evaluate(&obs, None, &Settings::default(), now).health,
        Health::Degraded
    );
    let unknown = observation(schedule(now, ""), now);
    assert_eq!(
        evaluate(&unknown, None, &Settings::default(), now).health,
        Health::Unknown
    );
    let mut fresh = observation(schedule(now, "UTC"), now);
    if let Data::Schedule {
        created_at,
        last_schedule,
        last_success,
        ..
    } = &mut fresh.data
    {
        *created_at = Some(now);
        *last_schedule = None;
        *last_success = None;
    }
    assert_eq!(
        evaluate(&fresh, None, &Settings::default(), now).health,
        Health::Unknown
    );
    Ok(())
}
#[test]
fn forbidden_overlap_keeps_a_running_job_in_progress() -> Result<(), Box<dyn std::error::Error>> {
    let now = at("2026-09-05T10:00:00Z")?;
    let mut obs = observation(schedule(now, "UTC"), now);
    if let Data::Schedule {
        active,
        forbid_overlap,
        ..
    } = &mut obs.data
    {
        *active = 1;
        *forbid_overlap = true;
    }
    assert_eq!(
        evaluate(&obs, None, &Settings::default(), now).health,
        Health::Unknown
    );
    Ok(())
}
#[test]
fn completed_job_updates_only_its_actual_owner() -> Result<(), Box<dyn std::error::Error>> {
    let now = at("2026-09-05T10:00:00Z")?;
    let mut result = CheckResult::failure(
        "dev".into(),
        Check::Kubernetes,
        "revision".into(),
        Coverage::Complete,
    );
    result.operations[0].id = "cronjobs".into();
    result
        .observations
        .push(observation(schedule(now, "UTC"), now));
    let mut owner = observation(
        Data::Owner {
            uid: "new-controller".into(),
            owner_uid: None,
        },
        now,
    );
    owner.resource.push_str("/owner");
    result.observations.push(owner);
    let mut job = observation(
        Data::Job {
            complete: true,
            failed: false,
            failed_attempts: 2,
            succeeded: 1,
            active: 0,
            created_at: Some(now - Duration::hours(1)),
            completed_at: Some(now - Duration::minutes(30)),
            scheduled_at: Some(now - Duration::hours(1)),
        },
        now,
    );
    job.resource = "dev/jobs/ns/nightly-123".into();
    result.observations.push(job);
    let mut owner = observation(
        Data::Owner {
            uid: "job".into(),
            owner_uid: Some("old-controller".into()),
        },
        now,
    );
    owner.resource = "dev/jobs/ns/nightly-123/owner".into();
    result.observations.push(owner);
    correlate(&mut result);
    assert!(
        matches!(result.observations[0].data,Data::Schedule{last_success:Some(at),..} if at < now-Duration::hours(1))
    );
    if let Data::Owner { owner_uid, .. } = &mut result.observations[3].data {
        *owner_uid = Some("new-controller".into());
    }
    correlate(&mut result);
    assert!(
        matches!(result.observations[0].data,Data::Schedule{last_success:Some(at),..} if at == now-Duration::minutes(30))
    );
    Ok(())
}
#[test]
fn daylight_saving_transition_does_not_shift_the_configured_hour()
-> Result<(), Box<dyn std::error::Error>> {
    let now = at("2026-11-01T17:00:00Z")?;
    let mut obs = observation(schedule(now, "America/Denver"), now);
    if let Data::Schedule {
        last_schedule,
        last_success,
        ..
    } = &mut obs.data
    {
        *last_schedule = Some(at("2026-11-01T16:00:00Z")?);
        *last_success = Some(at("2026-11-01T16:10:00Z")?);
    }
    assert_eq!(
        evaluate(&obs, None, &Settings::default(), now).health,
        Health::Healthy
    );
    Ok(())
}

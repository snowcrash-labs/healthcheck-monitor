//! Calendar-aware CronJob evaluation and UID-based completion correlation.
use crate::{
    config::settings::Settings,
    model::*,
    policy::{Evaluation, fault, result},
};
use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use croner::Cron;
use std::{collections::BTreeMap, str::FromStr};

/// The controller timezone must be known; workstation local time is never substituted.
pub fn evaluate(obs: &Observation, settings: &Settings, now: DateTime<Utc>) -> Evaluation {
    let Data::Schedule {
        schedule,
        timezone,
        suspended,
        active,
        last_schedule,
        last_success,
        created_at,
        starting_deadline_seconds,
        forbid_overlap,
    } = &obs.data
    else {
        return result(Health::Unknown);
    };
    if *suspended || obs.expected != Expected::Active {
        return result(Health::ExpectedInactive);
    }
    if *active > 0 && *forbid_overlap {
        return result(Health::Unknown);
    }
    if last_schedule > last_success
        && last_schedule
            .is_some_and(|at| (now - at).num_seconds() > settings.rollout_grace.0 as i64)
        && *active == 0
    {
        return fault(
            obs,
            "latest-schedule-not-successful",
            Severity::Warning,
            Confidence::Correlated,
        );
    }
    let (Ok(cron), Ok(zone)) = (Cron::from_str(schedule), Tz::from_str(timezone)) else {
        return result(Health::Unknown);
    };
    let grace = settings
        .rollout_grace
        .0
        .max(starting_deadline_seconds.unwrap_or(0).min(86400));
    let Some(cutoff) = now.checked_sub_signed(Duration::seconds(grace as i64)) else {
        return result(Health::Unknown);
    };
    let Ok(due) = cron.find_previous_occurrence(&cutoff.with_timezone(&zone), true) else {
        return result(Health::Unknown);
    };
    let due = due.to_utc();
    if created_at.is_some_and(|created| due < created) {
        return result(Health::Unknown);
    }
    if last_schedule.is_none_or(|at| at < due) {
        if created_at.is_none() && last_schedule.is_none() {
            return result(Health::Unknown);
        }
        return fault(
            obs,
            "schedule-missed",
            Severity::Warning,
            Confidence::Correlated,
        );
    }
    if last_success.is_some_and(|at| at >= due) {
        return result(Health::Healthy);
    }
    result(Health::Unknown)
}

/// A recreated controller cannot inherit a successful Job owned by its previous UID.
pub fn correlate(result: &mut CheckResult) {
    let mut owners = BTreeMap::new();
    for obs in &result.observations {
        if let Data::Owner { uid, owner_uid } = &obs.data
            && let Some(resource) = obs.resource.strip_suffix("/owner")
            && result
                .operations
                .iter()
                .any(|op| op.id == obs.operation && op.coverage == Coverage::Complete)
        {
            owners.insert(resource.to_owned(), (uid.clone(), owner_uid.clone()));
        }
    }
    let mut completions: BTreeMap<String, (Option<DateTime<Utc>>, DateTime<Utc>)> = BTreeMap::new();
    for obs in &result.observations {
        if let Data::Job {
            complete: true,
            failed: false,
            completed_at: Some(completed),
            scheduled_at,
            ..
        } = &obs.data
            && let Some((_, Some(owner))) = owners.get(&obs.resource)
            && result
                .operations
                .iter()
                .any(|op| op.id == obs.operation && op.coverage == Coverage::Complete)
        {
            let entry = completions
                .entry(owner.clone())
                .or_insert((*scheduled_at, *completed));
            entry.0 = entry.0.max(*scheduled_at);
            entry.1 = entry.1.max(*completed);
        }
    }
    for obs in &mut result.observations {
        if let Data::Schedule {
            last_success,
            last_schedule,
            ..
        } = &mut obs.data
            && let Some((uid, _)) = owners.get(&obs.resource)
            && let Some((scheduled, completed)) = completions.get(uid)
        {
            *last_success = (*last_success).max(Some(*completed));
            *last_schedule = (*last_schedule).max(*scheduled);
        }
    }
}

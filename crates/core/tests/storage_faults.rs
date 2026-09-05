//! Publication faults must preserve the last valid snapshot and unrelated files.
use chrono::{Duration, Utc};
use monitor_core::{
    config::{duration::Span, settings::Settings},
    state::State,
    storage::Store,
};
use std::{
    fs::{File, Permissions},
    os::unix::fs::PermissionsExt,
};
#[test]
fn interrupted_temporary_files_are_published_owner_only() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempfile::tempdir()?;
    let store = Store::open(directory.path())?;
    let temporary = directory.path().join(".monitor-latest.json.tmp");
    std::fs::write(&temporary, b"partial")?;
    std::fs::set_permissions(&temporary, Permissions::from_mode(0o644))?;
    store.publish(
        &State::new("test".into(), vec![]).snapshot,
        &[],
        &Settings::default(),
    )?;
    assert_eq!(
        std::fs::metadata(directory.path().join("monitor-latest.json"))?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    Ok(())
}
#[test]
fn retention_does_not_delete_similarly_named_operator_files()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = Store::open(directory.path())?;
    let note = directory
        .path()
        .join("monitor-snapshot-operator-notes.json");
    std::fs::write(&note, b"keep")?;
    File::open(&note)?.set_modified(std::time::UNIX_EPOCH)?;
    let settings = Settings {
        history_interval: Span(1),
        history_age: Span(1),
        ..Default::default()
    };
    store.publish(&State::new("test".into(), vec![]).snapshot, &[], &settings)?;
    assert!(note.exists());
    Ok(())
}
#[test]
fn failed_publication_keeps_previous_latest() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = Store::open(directory.path())?;
    let first = State::new("before".into(), vec![]).snapshot;
    store.publish(&first, &[], &Settings::default())?;
    std::fs::remove_file(directory.path().join("monitor-report.md"))?;
    std::fs::create_dir(directory.path().join("monitor-report.md"))?;
    let second = State::new("after".into(), vec![]).snapshot;
    assert!(store.publish(&second, &[], &Settings::default()).is_err());
    assert_eq!(
        store.latest(1024 * 1024)?.ok_or("missing latest")?.revision,
        "before"
    );
    Ok(())
}
#[test]
fn retention_bounds_all_published_service_artifacts() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = Store::open(directory.path())?;
    let settings = Settings {
        history_bytes: 2500,
        response_bytes: 1024,
        ..Default::default()
    };
    let mut snapshot = State::new("test".into(), vec![]).snapshot;
    for i in 0..10 {
        snapshot.captured_at = Utc::now() + Duration::seconds(i);
        store.publish(&snapshot, &[], &settings)?;
    }
    let total = std::fs::read_dir(directory.path())?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .map(|m| m.len())
        .sum::<u64>();
    assert!(total <= settings.history_bytes);
    Ok(())
}

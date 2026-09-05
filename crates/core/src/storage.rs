//! Owner-only atomic snapshots and service-owned bounded history.
use crate::{
    config::settings::Settings,
    error::Error,
    model::{Snapshot, Transition},
    report::markdown,
};
use chrono::{DateTime, Utc};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub struct Store {
    path: PathBuf,
    _lock: File,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self, Error> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        if fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Err(Error::Evidence);
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        let lock_path = path.join("monitor.lock");
        reject_symlink(&lock_path)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(lock_path)?;
        lock.set_permissions(fs::Permissions::from_mode(0o600))?;
        lock.try_lock().map_err(|_| Error::Locked)?;
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name
                .to_str()
                .and_then(|name| name.strip_prefix('.'))
                .and_then(|name| name.strip_suffix(".tmp"))
            else {
                continue;
            };
            if matches!(name, "monitor-latest.json" | "monitor-report.md") || owned(name).is_some()
            {
                reject_symlink(&entry.path())?;
                if entry.file_type()?.is_file() {
                    fs::remove_file(entry.path())?;
                }
            }
        }
        Ok(Self {
            path: path.to_path_buf(),
            _lock: lock,
        })
    }
    /// Each artifact is published only after fsync; a interrupted temporary file is ignored.
    pub fn publish(
        &self,
        snapshot: &Snapshot,
        transitions: &[Transition],
        settings: &Settings,
    ) -> Result<(), Error> {
        let encoded = serde_json::to_vec(snapshot)?;
        if encoded.len() > settings.memory_bytes / 2
            || encoded.len() as u64 > settings.history_bytes
        {
            return Err(Error::Evidence);
        }
        let stamp = snapshot
            .captured_at
            .format("%Y%m%dT%H%M%S%.9fZ")
            .to_string();
        let report = markdown(snapshot);
        let mut events = Vec::new();
        for transition in transitions {
            serde_json::to_writer(&mut events, transition)?;
            events.push(b'\n');
        }
        let reserve = encoded
            .len()
            .saturating_mul(2)
            .saturating_add(report.len())
            .saturating_add(events.len()) as u64;
        if reserve > settings.history_bytes {
            return Err(Error::Evidence);
        }
        let mut reserved = settings.clone();
        reserved.history_bytes = settings.history_bytes - reserve;
        self.retain(&reserved, snapshot.captured_at)?;
        self.atomic(&format!("monitor-snapshot-{stamp}.json"), &encoded)?;
        if !events.is_empty() {
            self.atomic(&format!("monitor-events-{stamp}.ndjson"), &events)?;
        }
        self.atomic("monitor-report.md", report.as_bytes())?;
        // The latest snapshot is the publication marker; leave it intact if any earlier write fails.
        self.atomic("monitor-latest.json", &encoded)?;
        self.retain(settings, snapshot.captured_at)?;
        Ok(())
    }
    fn atomic(&self, name: &str, bytes: &[u8]) -> Result<(), Error> {
        let target = self.path.join(name);
        let temp = self.path.join(format!(".{name}.tmp"));
        reject_symlink(&target)?;
        reject_symlink(&temp)?;
        let write = || -> Result<(), Error> {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&temp)?;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&temp, &target)?;
            File::open(&self.path)?.sync_all()?;
            Ok(())
        };
        let outcome = write();
        if outcome.is_err() {
            let _ = fs::remove_file(&temp);
        }
        outcome
    }
    pub fn latest(&self, limit: usize) -> Result<Option<Snapshot>, Error> {
        let latest = self.path.join("monitor-latest.json");
        reject_symlink(&latest)?;
        match read(&latest, limit) {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(_) => {
                let mut paths: Vec<_> = fs::read_dir(&self.path)?
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name()
                            .and_then(|s| s.to_str())
                            .is_some_and(|s| owned(s) == Some(true))
                    })
                    .take(10001)
                    .collect();
                paths.sort();
                if paths.is_empty() && !latest.exists() {
                    return Ok(None);
                }
                for path in paths.into_iter().rev() {
                    if let Ok(snapshot) = read(&path, limit) {
                        return Ok(Some(snapshot));
                    }
                }
                Err(Error::Evidence)
            }
        }
    }
    fn retain(&self, settings: &Settings, now: DateTime<Utc>) -> Result<(), Error> {
        let mut files = Vec::new();
        let mut bytes = ["monitor-latest.json", "monitor-report.md", "monitor.lock"]
            .iter()
            .filter_map(|name| fs::metadata(self.path.join(name)).ok())
            .map(|meta| meta.len())
            .sum::<u64>();
        for entry in fs::read_dir(&self.path)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(snapshot) = owned(&name) else {
                continue;
            };
            let meta = entry.metadata()?;
            if !meta.is_file() {
                continue;
            }
            bytes = bytes.saturating_add(meta.len());
            let stamp = name
                .trim_start_matches("monitor-snapshot-")
                .trim_start_matches("monitor-events-")
                .split('.')
                .next()
                .unwrap_or("");
            files.push((
                stamp.to_string(),
                entry.path(),
                meta.len(),
                snapshot,
                meta.modified()?,
            ));
            if files.len() > 20002 {
                return Err(Error::Evidence);
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        let mut count = files.iter().filter(|f| f.3).count();
        let now: std::time::SystemTime = now.into();
        for (_, path, size, snapshot, modified) in files {
            let age = now.duration_since(modified).unwrap_or_default();
            if bytes > settings.history_bytes
                || count > settings.history_count
                || age > settings.history_age.duration()
            {
                fs::remove_file(path)?;
                bytes = bytes.saturating_sub(size);
                if snapshot {
                    count = count.saturating_sub(1);
                }
            }
        }
        Ok(())
    }
}
fn reject_symlink(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(Error::Evidence),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Io(error)),
    }
}
fn owned(name: &str) -> Option<bool> {
    let (snapshot, stamp) = if let Some(stamp) = name
        .strip_prefix("monitor-snapshot-")
        .and_then(|s| s.strip_suffix(".json"))
    {
        (true, stamp)
    } else {
        let stamp = name
            .strip_prefix("monitor-events-")
            .and_then(|s| s.strip_suffix(".ndjson"))?;
        (false, stamp)
    };
    chrono::NaiveDateTime::parse_from_str(stamp, "%Y%m%dT%H%M%S%.fZ")
        .ok()
        .map(|_| snapshot)
}
pub fn read(path: &Path, limit: usize) -> Result<Snapshot, Error> {
    reject_symlink(path)?;
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(Error::Evidence);
    }
    let snapshot: Snapshot = serde_json::from_slice(&bytes)?;
    if snapshot.version != 1 {
        return Err(Error::Evidence);
    }
    Ok(snapshot)
}

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
        lock.try_lock().map_err(|_| Error::Locked)?;
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
        self.atomic("monitor-latest.json", &encoded)?;
        self.atomic("monitor-report.md", markdown(snapshot).as_bytes())?;
        self.atomic(&format!("monitor-snapshot-{stamp}.json"), &encoded)?;
        let mut events = Vec::new();
        for transition in transitions {
            serde_json::to_writer(&mut events, transition)?;
            events.push(b'\n');
        }
        if !events.is_empty() {
            self.atomic(&format!("monitor-events-{stamp}.ndjson"), &events)?;
        }
        self.retain(settings, snapshot.captured_at)?;
        Ok(())
    }
    fn atomic(&self, name: &str, bytes: &[u8]) -> Result<(), Error> {
        let target = self.path.join(name);
        let temp = self.path.join(format!(".{name}.tmp"));
        reject_symlink(&target)?;
        reject_symlink(&temp)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, target)?;
        File::open(&self.path)?.sync_all()?;
        Ok(())
    }
    pub fn latest(&self, limit: usize) -> Result<Option<Snapshot>, Error> {
        let latest = self.path.join("monitor-latest.json");
        if !latest.exists() {
            return Ok(None);
        }
        match read(&latest, limit) {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(_) => {
                let mut paths: Vec<_> = fs::read_dir(&self.path)?
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name().and_then(|s| s.to_str()).is_some_and(|s| {
                            s.starts_with("monitor-snapshot-") && s.ends_with(".json")
                        })
                    })
                    .take(10001)
                    .collect();
                paths.sort();
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
        let mut bytes = 0u64;
        for entry in fs::read_dir(&self.path)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !(name.starts_with("monitor-snapshot-") && name.ends_with(".json")
                || name.starts_with("monitor-events-") && name.ends_with(".ndjson"))
            {
                continue;
            }
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
                name.starts_with("monitor-snapshot-"),
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

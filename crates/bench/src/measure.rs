//! Wall time and waited process-tree CPU are measured outside the benchmarked process.
use crate::{Error, Variant, legacy, memory};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Serialize)]
pub struct Measurement {
    pub wall_seconds: f64,
    pub user_seconds: f64,
    pub system_seconds: f64,
    pub peak_tree_rss_bytes: u64,
    pub peak_root_rss_bytes: u64,
    pub peak_processes: usize,
    pub observed_processes: usize,
    pub memory_samples: usize,
    pub sampling_incomplete: bool,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
    pub legacy: Option<serde_json::Value>,
}

pub fn private_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

struct Child {
    pid: i32,
    reaped: bool,
}
impl Drop for Child {
    fn drop(&mut self) {
        if !self.reaped {
            // The original leader is still owned; kill its group before reaping it.
            unsafe {
                libc::kill(-self.pid, libc::SIGKILL);
                libc::waitpid(self.pid, std::ptr::null_mut(), 0);
            }
        }
    }
}

pub fn run(variant: &Variant, output: &Path, timeout: u64) -> Result<Measurement, Error> {
    std::fs::create_dir(output)?;
    std::fs::set_permissions(output, std::fs::Permissions::from_mode(0o700))?;
    let sink = variant
        .legacy_json
        .as_deref()
        .map(|name| legacy::Sink::start(output, name))
        .transpose()?;
    let command: Vec<_> = variant
        .command
        .iter()
        .map(|part| part.replace("{output}", &output.to_string_lossy()))
        .collect();
    let mut builder = Command::new(command.first().ok_or("empty benchmark command")?);
    builder
        .args(&command[1..])
        .process_group(0)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(Stdio::null())
        .stdout(private_file(&output.join("stdout.txt"))?)
        .stderr(private_file(&output.join("stderr.txt"))?);
    let started = Instant::now();
    let child = builder.spawn()?;
    let mut owned = Child {
        pid: i32::try_from(child.id())?,
        reaped: false,
    };
    drop(child);
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    let mut status = 0;
    let mut result = Measurement {
        wall_seconds: 0.0,
        user_seconds: 0.0,
        system_seconds: 0.0,
        peak_tree_rss_bytes: 0,
        peak_root_rss_bytes: 0,
        peak_processes: 0,
        observed_processes: 0,
        memory_samples: 0,
        sampling_incomplete: false,
        exit_code: None,
        signal: None,
        timed_out: false,
        legacy: None,
    };
    let mut seen = BTreeSet::new();
    loop {
        // wait4 returns resource use including descendants the command waited for.
        let waited =
            unsafe { libc::wait4(owned.pid, &mut status, libc::WNOHANG, usage.as_mut_ptr()) };
        if waited == owned.pid {
            owned.reaped = true;
            break;
        }
        if waited < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        let sample = memory::tree(owned.pid);
        result.memory_samples += 1;
        result.peak_tree_rss_bytes = result.peak_tree_rss_bytes.max(sample.rss);
        result.peak_root_rss_bytes = result.peak_root_rss_bytes.max(sample.root_rss);
        result.peak_processes = result.peak_processes.max(sample.processes);
        result.sampling_incomplete |= sample.incomplete;
        for pid in &sample.pids {
            if seen.len() < 4096 {
                seen.insert(*pid);
            } else {
                result.sampling_incomplete = true;
            }
        }
        if started.elapsed() > Duration::from_secs(timeout)
            || crate::CANCELLED.load(std::sync::atomic::Ordering::Relaxed)
        {
            result.timed_out = started.elapsed() > Duration::from_secs(timeout);
            // Native credential helpers may use their own process groups.
            for pid in sample.pids.iter().rev().filter(|pid| **pid != owned.pid) {
                unsafe {
                    libc::kill(*pid, libc::SIGKILL);
                }
            }
            unsafe {
                libc::kill(-owned.pid, libc::SIGKILL);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    result.wall_seconds = started.elapsed().as_secs_f64();
    let usage = unsafe { usage.assume_init() };
    result.user_seconds = seconds(usage.ru_utime);
    result.system_seconds = seconds(usage.ru_stime);
    result.observed_processes = seen.len();
    if libc::WIFEXITED(status) {
        result.exit_code = Some(libc::WEXITSTATUS(status));
    }
    if libc::WIFSIGNALED(status) {
        result.signal = Some(libc::WTERMSIG(status));
    }
    if let Some(sink) = sink {
        result.legacy = Some(sink.finish()?);
    }
    Ok(result)
}

fn seconds(value: libc::timeval) -> f64 {
    value.tv_sec as f64 + value.tv_usec as f64 / 1_000_000.0
}

#[cfg(test)]
mod tests {
    fn output() -> Result<std::path::PathBuf, crate::Error> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        Ok(std::env::temp_dir().join(format!("monitor-bench-{}-{stamp}", std::process::id())))
    }

    #[test]
    fn reports_nonzero_exit_without_failing_the_measurement() -> Result<(), crate::Error> {
        let output = output()?;
        let measured = super::run(
            &crate::Variant {
                name: "fixture".into(),
                command: vec!["/bin/sh".into(), "-c".into(), "sleep 0.05; exit 7".into()],
                legacy_json: None,
            },
            &output,
            2,
        )?;
        std::fs::remove_dir_all(output)?;
        assert_eq!(measured.exit_code, Some(7));
        assert!(!measured.timed_out);
        assert!(measured.wall_seconds >= 0.05);
        assert!(measured.peak_tree_rss_bytes > 0);
        Ok(())
    }

    #[test]
    fn deadline_terminates_and_reaps_a_process_group() -> Result<(), crate::Error> {
        let output = output()?;
        let measured = super::run(
            &crate::Variant {
                name: "fixture".into(),
                command: vec!["/bin/sh".into(), "-c".into(), "sleep 5 & wait".into()],
                legacy_json: None,
            },
            &output,
            1,
        )?;
        std::fs::remove_dir_all(output)?;
        assert!(measured.timed_out);
        assert_eq!(measured.signal, Some(libc::SIGKILL));
        assert!(measured.wall_seconds < 3.0);
        Ok(())
    }
}

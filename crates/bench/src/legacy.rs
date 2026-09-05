//! Drain original Python JSON through a FIFO so workload values never reach disk.
use crate::Error;
use serde_json::{Value, json};
use std::{
    io::Read,
    os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub struct Sink {
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<Result<Value, Error>>>,
}

impl Sink {
    pub fn start(output: &Path, filename: &str) -> Result<Self, Error> {
        if !matches!(filename, "kubernetes.json" | "queues.json") {
            return Err("unsupported legacy JSON sink".into());
        }
        let path = output.join(format!("{filename}.tmp"));
        let name = std::ffi::CString::new(path.as_os_str().as_bytes())?;
        if unsafe { libc::mkfifo(name.as_ptr(), 0o600) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let worker = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut buffer = [0; 65536];
            loop {
                match file.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        if bytes.len() + count > 128 * 1024 * 1024 {
                            return Err("legacy evidence exceeds 128 MiB".into());
                        }
                        bytes.extend_from_slice(&buffer[..count]);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if flag.load(Ordering::Acquire) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            let value: Value = serde_json::from_slice(&bytes)?;
            Ok(summarize(&value, bytes.len()))
        });
        Ok(Self {
            stop,
            worker: Some(worker),
        })
    }

    pub fn finish(mut self) -> Result<Value, Error> {
        self.stop.store(true, Ordering::Release);
        self.worker
            .take()
            .ok_or("missing FIFO worker")?
            .join()
            .map_err(|_| -> Error { "FIFO worker stopped unexpectedly".into() })?
    }
}

impl Drop for Sink {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn summarize(value: &Value, bytes: usize) -> Value {
    let mut counts = std::collections::BTreeMap::new();
    if let Some(environments) = value.get("environments").and_then(Value::as_object) {
        for (name, environment) in environments {
            for key in [
                "workloads",
                "pods",
                "nodes",
                "scaledobjects",
                "warning_events",
            ] {
                if let Some(items) = environment
                    .get(key)
                    .and_then(|v| v.get("items"))
                    .and_then(Value::as_array)
                {
                    counts.insert(format!("{name}/{key}"), items.len());
                }
            }
            if let Some(details) = environment.get("details").and_then(Value::as_array) {
                counts.insert(format!("{name}/queue_details"), details.len());
                counts.insert(
                    format!("{name}/queue_details_ok"),
                    details
                        .iter()
                        .filter(|v| v.get("value").is_some_and(|value| !value.is_null()))
                        .count(),
                );
            }
        }
    }
    let operations: Vec<_> = value.get("commands").and_then(Value::as_array).into_iter().flatten()
        .map(|op| json!({"name":op.get("name"),"exit_code":op.get("exit_code"),"timed_out":op.get("timed_out"),"duration_ms":op.get("duration_ms")})).collect();
    json!({"serialized_bytes":bytes,"counts":counts,"operations":operations,"sample_count":value.get("sample_count")})
}

#[cfg(test)]
mod tests {
    #[test]
    fn atomic_fifo_publication_retains_only_metadata() -> Result<(), crate::Error> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let output =
            std::env::temp_dir().join(format!("monitor-bench-fifo-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&output)?;
        let sink = super::Sink::start(&output, "kubernetes.json")?;
        std::fs::write(
            output.join("kubernetes.json.tmp"),
            br#"{"environments":{"dev":{"pods":{"items":[{"value":"private"}]}}},"commands":[]}"#,
        )?;
        std::fs::rename(
            output.join("kubernetes.json.tmp"),
            output.join("kubernetes.json"),
        )?;
        let result = sink.finish()?;
        use std::os::unix::fs::FileTypeExt;
        assert!(
            std::fs::metadata(output.join("kubernetes.json"))?
                .file_type()
                .is_fifo()
        );
        std::fs::remove_dir_all(output)?;
        assert_eq!(result["counts"]["dev/pods"], 1);
        assert!(!result.to_string().contains("private"));
        Ok(())
    }

    #[test]
    fn projection_discards_workload_values_and_error_text() {
        let value = serde_json::json!({"environments":{"dev":{"pods":{"items":[{"env":"private"}]}}},"commands":[{"name":"pods","stderr":"private","argv":["private"],"exit_code":0}]});
        let safe = super::summarize(&value, 100).to_string();
        assert!(!safe.contains("private"));
        assert!(safe.contains("dev/pods"));
    }
}

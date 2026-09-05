//! Repeated command measurements with descendant RSS and metadata-only legacy evidence.
mod legacy;
mod measure;
mod memory;

use serde::{Deserialize, Serialize};
use std::{io::Write, path::PathBuf};
type Error = Box<dyn std::error::Error + Send + Sync>;
static CANCELLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn cancel(_: libc::c_int) {
    CANCELLED.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    output: PathBuf,
    warmups: usize,
    runs: usize,
    timeout_seconds: u64,
    variants: Vec<Variant>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Variant {
    name: String,
    command: Vec<String>,
    legacy_json: Option<String>,
}

#[derive(Serialize)]
struct Trial {
    variant: String,
    warmup: bool,
    round: usize,
    output: PathBuf,
    #[serde(flatten)]
    measurement: measure::Measurement,
}

fn run() -> Result<(), Error> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: monitor-bench PLAN.toml")?;
    let text = std::fs::read_to_string(path)?;
    if text.len() > 65_536 {
        return Err("benchmark plan exceeds 64 KiB".into());
    }
    let plan: Plan = toml::from_str(&text)?;
    if plan.runs == 0
        || plan.runs > 20
        || plan.warmups > 3
        || plan.variants.is_empty()
        || plan.variants.len() > 8
        || plan.timeout_seconds == 0
        || plan.timeout_seconds > 600
    {
        return Err("benchmark bounds are invalid".into());
    }
    for variant in &plan.variants {
        if variant.command.is_empty()
            || variant.command.len() > 64
            || !variant
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("invalid command or variant name".into());
        }
    }
    std::fs::create_dir_all(&plan.output)?;
    let path = plan.output.join("measurements.ndjson");
    let mut results = measure::private_file(&path)?;
    for round in 0..plan.warmups + plan.runs {
        let mut variants: Vec<_> = plan.variants.iter().collect();
        // Alternating order distributes provider cache and background-load drift.
        if round % 2 == 1 {
            variants.reverse();
        }
        for variant in variants {
            if CANCELLED.load(std::sync::atomic::Ordering::Relaxed) {
                return Err("benchmark cancelled".into());
            }
            let output = plan.output.join(format!("{}-{round:02}", variant.name));
            let trial = Trial {
                variant: variant.name.clone(),
                warmup: round < plan.warmups,
                round,
                measurement: measure::run(variant, &output, plan.timeout_seconds)?,
                output,
            };
            serde_json::to_writer(&mut results, &trial)?;
            writeln!(&mut results)?;
            results.sync_data()?;
            serde_json::to_writer(std::io::stdout().lock(), &trial)?;
            writeln!(std::io::stdout().lock())?;
        }
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    for signal in [libc::SIGINT, libc::SIGTERM] {
        // The handler only writes an atomic flag; the sampling loop performs cleanup.
        if unsafe { libc::signal(signal, cancel as *const () as libc::sighandler_t) }
            == libc::SIG_ERR
        {
            return std::process::ExitCode::FAILURE;
        }
    }
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(std::io::stderr().lock(), "Benchmark failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

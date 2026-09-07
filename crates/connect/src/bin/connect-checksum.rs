//! Write a deterministic checksum for a connector distribution artifact.
use clap::Parser;
use sha2::Digest;
use std::{io::Read, path::PathBuf};
#[derive(Parser)]
struct Args {
    binary: PathBuf,
    output: PathBuf,
}
fn main() -> std::process::ExitCode {
    let _ = tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .try_init();
    match checksum(Args::parse()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(_) => {
            tracing::error!("Connector checksum could not be written");
            std::process::ExitCode::FAILURE
        }
    }
}
fn checksum(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = std::fs::File::open(&args.binary)?;
    if !file.metadata()?.is_file() {
        return Err("not a regular binary file".into());
    }
    let mut hash = sha2::Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    let name = args
        .binary
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("invalid file name")?;
    if name.chars().any(char::is_control) {
        return Err("invalid file name".into());
    }
    std::fs::write(args.output, format!("{digest}  {name}\n"))?;
    Ok(())
}

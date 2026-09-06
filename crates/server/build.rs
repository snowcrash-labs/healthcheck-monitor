//! Precompress the Vite output before rust-embed incorporates it into the executable.
use flate2::{Compression, write::GzEncoder};
use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=HEALTHCHECK_ASSETS_DIR");
    let workspace = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("../..");
    let source = env::var_os("HEALTHCHECK_ASSETS_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                workspace.join(path)
            }
        })
        .unwrap_or_else(|| workspace.join("dashboard/dist"));
    if !source.join("index.html").is_file() {
        return Err("Build dashboard assets with npm --prefix dashboard run build before compiling the server".into());
    }
    if fs::symlink_metadata(&source)?.file_type().is_symlink() {
        return Err("Dashboard root cannot be a symlink".into());
    }
    let destination = PathBuf::from(env::var("OUT_DIR")?).join("dashboard");
    if destination.exists() {
        fs::remove_dir_all(&destination)?;
    }
    fs::create_dir_all(&destination)?;
    let mut pending = vec![source.clone()];
    let mut paths = Vec::new();
    let mut count = 0usize;
    while let Some(directory) = pending.pop() {
        println!("cargo:rerun-if-changed={}", directory.display());
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            count += 1;
            if count > 2048 {
                return Err("Dashboard asset count exceeds its bound".into());
            }
            if entry.file_type()?.is_symlink() {
                return Err("Dashboard assets cannot contain symlinks".into());
            }
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            } else if entry.file_type()?.is_file() {
                paths.push(entry.path());
            } else {
                return Err("Dashboard assets must be regular files".into());
            }
            if pending.len() + paths.len() > 2048 {
                return Err("Dashboard asset count exceeds its bound".into());
            }
        }
    }
    paths.sort();
    let mut total = 0usize;
    for path in paths {
        println!("cargo:rerun-if-changed={}", path.display());
        let mut raw = Vec::new();
        fs::File::open(&path)?
            .take((64 * 1024 * 1024 - total + 1) as u64)
            .read_to_end(&mut raw)?;
        total += raw.len();
        if total > 64 * 1024 * 1024 {
            return Err("Dashboard bytes exceed their bound".into());
        }
        let relative = path.strip_prefix(&source)?;
        write(&destination.join("raw").join(relative), &raw)?;
        let mut gzip = GzEncoder::new(Vec::new(), Compression::best());
        gzip.write_all(&raw)?;
        write(&destination.join("gzip").join(relative), &gzip.finish()?)?;
        write(
            &destination.join("zstd").join(relative),
            &zstd::stream::encode_all(raw.as_slice(), 19)?,
        )?;
    }
    Ok(())
}
fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)
}

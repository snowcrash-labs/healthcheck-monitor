//! Rebuild embedded migrations when SQL changes without a Rust source change.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}

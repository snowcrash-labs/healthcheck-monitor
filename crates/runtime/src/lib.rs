//! Shared foreground monitoring lifecycle for CLI and HTTP supervision.
mod supervise;
use monitor_core::{
    config::resolve::{Effective, Selection},
    model::{Snapshot, Transition},
};
pub use supervise::{load, monitor};

pub struct Options {
    pub selection: Selection,
    pub output: std::path::PathBuf,
    pub strict: bool,
}
/// Observers receive bounded read views; they never own or drive the scheduler.
pub trait Observer: Send + Sync + 'static {
    fn update(&self, snapshot: &Snapshot, effective: &Effective, transitions: &[Transition]);
    fn heartbeat(&self, at: chrono::DateTime<chrono::Utc>, running: bool);
}
pub struct Noop;
impl Observer for Noop {
    fn update(&self, _: &Snapshot, _: &Effective, _: &[Transition]) {}
    fn heartbeat(&self, _: chrono::DateTime<chrono::Utc>, _: bool) {}
}

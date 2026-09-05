//! CLI argument adaptation for the shared monitoring lifecycle.
pub use monitor_runtime::load;
pub async fn monitor(
    config: &std::path::Path,
    options: crate::args::Options,
    mode: monitor_core::scheduler::Mode,
) -> Result<u8, monitor_core::error::Error> {
    monitor_runtime::monitor(
        config,
        monitor_runtime::Options {
            selection: options.selection.selection(),
            output: options.output,
            strict: options.strict,
        },
        mode,
        std::sync::Arc::new(monitor_runtime::Noop),
        tokio_util::sync::CancellationToken::new(),
    )
    .await
}

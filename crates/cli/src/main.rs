//! Foreground read-only monitoring command.
mod args;
mod commands;
mod runtime;
use clap::Parser;
use std::process::ExitCode;
#[global_allocator]
static ALLOCATOR: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;
fn main() -> ExitCode {
    // Fixed filters prevent SDK diagnostics from exposing credentials or payloads.
    tracing_subscriber::fmt().json().with_current_span(true).with_span_list(false).flatten_event(true).with_ansi(false)
        .with_env_filter("off,healthcheck_monitor=info,monitor_core=info,monitor_integrations=info,monitor_providers=info,monitor_runtime=info,monitor_server=info,monitor_history=info").init();
    // SDK panic payloads may contain provider data; collection records only the operation failure.
    std::panic::set_hook(Box::new(|info| match info.location() {
        Some(location) => tracing::error!(
            file = location.file(),
            line = location.line(),
            "Internal task failure"
        ),
        None => tracing::error!("Internal task failure"),
    }));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        // Generated cloud SDK futures need more stack in development builds than Tokio's default.
        .thread_stack_size(16 * 1024 * 1024)
        .max_blocking_threads(16)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            tracing::error!("Unable to initialize async runtime");
            return ExitCode::from(2);
        }
    };
    let result = runtime.block_on(commands::execute(args::Cli::parse()));
    // A stalled filesystem cannot hold foreground shutdown indefinitely.
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            tracing::error!(error = %error, "Command failed");
            ExitCode::from(2)
        }
    }
}

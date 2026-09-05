//! Foreground read-only monitoring command.
mod args;
mod commands;
mod runtime;
use clap::Parser;
use std::process::ExitCode;
#[global_allocator]
static ALLOCATOR: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;
#[tokio::main]
async fn main() -> ExitCode {
    // Fixed filters prevent SDK diagnostics from exposing credentials or payloads.
    tracing_subscriber::fmt().json().with_current_span(true).with_span_list(false).flatten_event(true).with_ansi(false)
        .with_env_filter("off,healthcheck_monitor=info,monitor_core=info,monitor_integrations=info,monitor_providers=info").init();
    match commands::execute(args::Cli::parse()).await {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            tracing::error!(error = %error, "Command failed");
            ExitCode::from(2)
        }
    }
}

//! A local MCP connector queries the continuous monitor using each operator's Google identity.
mod auth;
mod client;
mod config;
mod credentials;
mod error;
mod google;
mod input;
#[cfg(test)]
mod live_tests;
mod login;
mod mcp;
#[cfg(test)]
mod security_tests;
#[cfg(test)]
mod tests;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::util::SubscriberInitExt;

#[global_allocator]
static ALLOCATOR: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;
#[derive(Parser)]
#[command(
    version,
    about = "Query the continuous health monitor through Google IAP"
)]
struct Args {
    /// Connection profile containing only the server and desktop OAuth client configuration.
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Import the administrator-provided Google desktop client configuration.
    Configure {
        #[arg(long)]
        client_json: PathBuf,
    },
    Login {
        #[arg(long)]
        no_browser: bool,
        #[arg(long,value_parser=clap::value_parser!(u16).range(1024..))]
        callback_port: Option<u16>,
    },
    Status,
    Logout,
    Mcp,
}
#[tokio::main]
async fn main() -> std::process::ExitCode {
    // Stdout belongs exclusively to the MCP protocol while serving tools.
    let _ = tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .with_env_filter("healthcheck_connect=info")
        .finish()
        .try_init();
    match run(Args::parse()).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(error=%error,"Connector operation failed");
            std::process::ExitCode::FAILURE
        }
    }
}
async fn run(args: Args) -> Result<(), error::Error> {
    if let Command::Configure { client_json } = &args.command {
        return config::Config::import(args.config, client_json).await;
    }
    let config = config::Config::load(args.config).await?;
    let client = client::Client::new(config)?;
    match args.command {
        Command::Configure { .. } => Err(error::Error::Configuration),
        Command::Login {
            no_browser,
            callback_port,
        } => login::login(&client, no_browser, callback_port).await,
        Command::Status => {
            let credentials = client.store.load().await?;
            client
                .get::<_, monitor_query::response::Page<monitor_query::response::ScopeInfo>>(
                    "scopes",
                    &monitor_query::filter::Filter::default(),
                )
                .await?;
            tracing::info!(email=%credentials.email,"Google identity has monitoring access");
            Ok(())
        }
        Command::Logout => client.logout().await,
        Command::Mcp => mcp::serve(client).await,
    }
}

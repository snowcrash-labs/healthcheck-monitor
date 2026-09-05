//! Command selection over the shared configuration resolver.
use clap::{Args, Parser, Subcommand};
use monitor_core::{
    config::{duration::Span, resolve::Selection, settings::SettingsPatch},
    model::Check,
};
use std::path::PathBuf;
#[derive(Parser)]
#[command(
    name = "healthcheck-monitor",
    version,
    about = "Read-only monitoring of explicitly selected infrastructure"
)]
pub struct Cli {
    #[arg(long, global = true, default_value = "monitor.toml")]
    pub config: PathBuf,
    #[command(subcommand)]
    pub command: Command,
}
#[derive(Subcommand)]
pub enum Command {
    Run(Options),
    Watch(WatchOptions),
    Serve(ServeOptions),
    Report {
        snapshot: PathBuf,
    },
    Diff {
        older: PathBuf,
        newer: PathBuf,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
}
#[derive(Subcommand)]
pub enum ConfigCommand {
    Validate {
        #[arg(long)]
        show_effective: bool,
        #[command(flatten)]
        selection: Select,
    },
}
#[derive(Subcommand)]
pub enum AuthCommand {
    Status,
    Login { profile: String },
}
#[derive(Args, Clone, Default)]
pub struct Select {
    #[arg(long)]
    pub profile: Option<String>,
    #[arg(long = "target", value_delimiter = ',')]
    pub targets: Vec<String>,
    #[arg(long="check", value_delimiter=',', value_parser=parse_check)]
    pub checks: Vec<Check>,
    #[arg(long = "resource", value_delimiter = ',')]
    pub resources: Vec<String>,
    #[arg(long)]
    pub samples: Option<u32>,
    #[arg(long)]
    pub interval: Option<Span>,
}
#[derive(Args)]
pub struct Options {
    #[command(flatten)]
    pub selection: Select,
    #[arg(long, default_value = "evidence")]
    pub output: PathBuf,
    #[arg(long)]
    pub strict: bool,
}
#[derive(Args)]
pub struct WatchOptions {
    #[command(flatten)]
    pub options: Options,
    #[arg(long)]
    pub duration: Option<Span>,
}
#[derive(Args)]
pub struct ServeOptions {
    #[command(flatten)]
    pub watch: WatchOptions,
    #[arg(long)]
    pub server_config: Option<PathBuf>,
    #[arg(long)]
    pub listen: Option<std::net::SocketAddr>,
    #[arg(long)]
    pub assets: Option<PathBuf>,
}
impl Select {
    pub fn selection(&self) -> Selection {
        Selection {
            profile: self.profile.clone(),
            targets: self.targets.clone(),
            checks: self.checks.clone(),
            resources: self.resources.clone(),
            overrides: SettingsPatch {
                samples: self.samples,
                interval: self.interval,
                ..Default::default()
            },
        }
    }
}
fn parse_check(value: &str) -> Result<Check, String> {
    serde_json::from_value(serde_json::Value::String(value.into()))
        .map_err(|_| "unknown check name".into())
}

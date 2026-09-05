//! Command dispatch and offline evidence tools.
use crate::args::{AuthCommand, Cli, Command, ConfigCommand};
use monitor_core::{
    config::resolve::Selection, error::Error, model::Check, scheduler::Mode, storage,
};
pub async fn execute(cli: Cli) -> Result<u8, Error> {
    match cli.command {
        Command::Run(options) => crate::runtime::monitor(&cli.config, options, Mode::Once).await,
        Command::Watch(options) => {
            crate::runtime::monitor(
                &cli.config,
                options.options,
                Mode::Watch {
                    duration: options.duration.map(|d| d.duration()),
                },
            )
            .await
        }
        Command::Report { snapshot } => {
            let snapshot = storage::read(&snapshot, 128 * 1024 * 1024)?;
            print!("{}", monitor_core::report::markdown(&snapshot));
            Ok(0)
        }
        Command::Diff { older, newer } => {
            let old = storage::read(&older, 128 * 1024 * 1024)?;
            let new = storage::read(&newer, 128 * 1024 * 1024)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&monitor_core::report::diff(&old, &new))?
            );
            Ok(0)
        }
        Command::Config {
            command:
                ConfigCommand::Validate {
                    show_effective,
                    selection,
                },
        } => {
            let config = crate::runtime::load(&cli.config)?;
            let effective = config.resolve(&selection.selection())?;
            if show_effective {
                println!("{}", serde_json::to_string_pretty(&effective)?);
            } else {
                tracing::info!(checks = effective.jobs.len(), revision = %effective.revision, "Configuration valid");
            }
            Ok(0)
        }
        Command::Auth {
            command: AuthCommand::Status,
        } => {
            let config = crate::runtime::load(&cli.config)?;
            let effective = config.resolve(&Selection {
                checks: vec![Check::Preflight],
                ..Default::default()
            })?;
            let router = monitor_providers::router::Router::new(config, 2);
            use monitor_core::scheduler::Collector;
            let mut code = 0;
            for job in &effective.jobs {
                let result = router
                    .collect(job, tokio_util::sync::CancellationToken::new())
                    .await;
                tracing::info!(target_name = %job.target.name, authenticated = result.complete(), "Authentication status");
                if !result.complete() {
                    code = 3;
                }
            }
            Ok(code)
        }
        Command::Auth {
            command: AuthCommand::Login { profile },
        } => {
            let config = crate::runtime::load(&cli.config)?;
            let credential = config
                .credentials
                .get(&profile)
                .ok_or_else(|| Error::Config("unknown credential profile".into()))?;
            let helper = monitor_integrations::process::Helper::Login {
                credential: credential.clone(),
            };
            monitor_integrations::process::Processes::new(1)
                .run(
                    helper,
                    64 * 1024,
                    std::time::Duration::from_secs(300),
                    &tokio_util::sync::CancellationToken::new(),
                )
                .await
                .map_err(|_| Error::Config("authentication helper failed".into()))?;
            Ok(0)
        }
    }
}

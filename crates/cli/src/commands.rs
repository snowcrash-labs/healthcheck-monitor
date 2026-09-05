//! Command dispatch and offline evidence tools.
use crate::args::{AuthCommand, Cli, Command, ConfigCommand};
use monitor_core::{
    config::resolve::Selection, error::Error, model::Check, scheduler::Mode, storage,
};
pub async fn execute(cli: Cli) -> Result<u8, Error> {
    match cli.command {
        Command::Run(options) => crate::runtime::monitor(&cli.config, options, Mode::Once).await,
        Command::Serve(options) => {
            let mut config = monitor_server::config::Config::load(options.server_config.as_deref())
                .map_err(|_| Error::Config("invalid dashboard configuration".into()))?;
            if let Some(listen) = options.listen {
                config.listen = listen;
            }
            if let Some(assets) = options.assets {
                config.assets = assets;
            }
            let selected = options.watch.options;
            monitor_server::serve(
                &cli.config,
                monitor_runtime::Options {
                    selection: selected.selection.selection(),
                    output: selected.output,
                    strict: selected.strict,
                },
                options.watch.duration.map(|duration| duration.duration()),
                config,
            )
            .await
            .map_err(|error| Error::Config(error.to_string()))
        }
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
            output(&monitor_core::report::markdown(&snapshot))?;
            Ok(0)
        }
        Command::Diff { older, newer } => {
            let old = storage::read(&older, 128 * 1024 * 1024)?;
            let new = storage::read(&newer, 128 * 1024 * 1024)?;
            output(&format!(
                "{}\n",
                serde_json::to_string_pretty(&monitor_core::report::diff(&old, &new))?
            ))?;
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
                output(&format!("{}\n", serde_json::to_string_pretty(&effective)?))?;
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
            let router = monitor_providers::router::Router::new(config, &effective);
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
            monitor_integrations::process::Processes::new(1)
                .login(credential.clone(), std::time::Duration::from_secs(300))
                .await
                .map_err(|_| Error::Config("authentication helper failed".into()))?;
            Ok(0)
        }
    }
}
fn output(text: &str) -> Result<(), Error> {
    use std::io::Write;
    std::io::stdout().lock().write_all(text.as_bytes())?;
    Ok(())
}

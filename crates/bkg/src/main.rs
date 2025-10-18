mod api;
mod cli;
mod config;
mod plugins;
mod sandbox;
mod telemetry;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use tokio::signal;
use tracing_subscriber::EnvFilter;

use crate::api::ApiServer;
use crate::cli::{Cli, Commands, PluginCommand, ServerSubcommand};
use crate::config::AppConfig;
use crate::plugins::PluginManager;
use crate::sandbox::SandboxManager;
use crate::telemetry::TelemetryGuard;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    init_tracing(cli.verbose);

    match &cli.command {
        Commands::Server(server_cmd) => match &server_cmd.command {
            ServerSubcommand::Start {
                config,
                watch_plugins,
            } => run_server(config.as_ref(), *watch_plugins).await,
            ServerSubcommand::HealthCheck { config } => {
                let config = AppConfig::load(config.as_ref())?;
                println!(
                    "healthz endpoint configured at: {}:{}{}",
                    config.server.bind_address, config.server.port, config.server.health_path
                );
                Ok(())
            }
        },
        Commands::Plugin(PluginCommand::List { config }) => {
            let config = AppConfig::load(config.as_ref())?;
            let manager = PluginManager::new(config.plugins.clone());
            manager.preview(config.plugin_directory.clone())?;
            Ok(())
        }
        Commands::Sandbox(sandbox_cmd) => sandbox_cmd.execute().await,
    }
}

fn init_tracing(verbosity: u8) {
    let filter = match verbosity {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(filter.parse().unwrap()))
        .with_target(false)
        .init();
}

async fn run_server(config_path: Option<&PathBuf>, watch_plugins: bool) -> Result<()> {
    let config = AppConfig::load(config_path)?;

    let telemetry = TelemetryGuard::initialize(&config.telemetry);
    let sandbox_manager = Arc::new(SandboxManager::default());
    let plugin_manager = Arc::new(PluginManager::new(config.plugins.clone()));

    plugin_manager
        .load_from_directory(
            &config.plugin_directory,
            &config.namespace,
            &config.environment,
        )
        .await?;

    let api = ApiServer::new(
        config.server.clone(),
        sandbox_manager.clone(),
        plugin_manager.clone(),
    );

    let server_task = tokio::spawn(api.serve());

    if watch_plugins {
        let plugin_dir = config.plugin_directory.clone();
        let namespace = config.namespace.clone();
        let environment = config.environment.clone();
        let manager = plugin_manager.clone();
        tokio::spawn(async move {
            if let Err(err) = manager
                .watch_directory(plugin_dir, namespace, environment)
                .await
            {
                tracing::error!("plugin watcher error: {err}");
            }
        });
    }

    signal::ctrl_c().await?;
    tracing::info!("received shutdown signal");

    plugin_manager.shutdown_all().await;
    telemetry.shutdown();

    server_task.abort();

    Ok(())
}

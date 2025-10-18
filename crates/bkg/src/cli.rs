use std::path::PathBuf;

use crate::sandbox::{ResourceLimits, SandboxConfig, SandboxManager, SandboxRuntime};
use anyhow::{Context, Result};
use clap::{ArgAction, Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(author, version, about = "BKG – universelle KI-Infrastruktur CLI")]
pub struct Cli {
    /// Verbosity level (-v, -vv).
    #[arg(short, long, action = ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Control the server lifecycle.
    Server(ServerCommand),
    /// Interact with the plugin subsystem.
    Plugin(PluginCommand),
    /// Perform simple sandbox workflows locally.
    Sandbox(SandboxCommand),
}

#[derive(Debug, Args)]
pub struct ServerCommand {
    #[command(subcommand)]
    pub command: ServerSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ServerSubcommand {
    /// Start the API server and plugin runtime.
    Start {
        /// Optional path to `cave.yaml` configuration.
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Watch the plugin directory for changes and hot-reload libraries.
        #[arg(long)]
        watch_plugins: bool,
    },
    /// Print the configured health endpoint.
    HealthCheck {
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// List plugins discovered on disk without starting the server.
    List {
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum SandboxCommand {
    /// Create a sandbox configuration locally and print its identifier.
    Create {
        #[arg(long)]
        project: String,
        #[arg(long)]
        runtime: SandboxRuntime,
        #[arg(long, default_value_t = 512)]
        memory_mb: u64,
    },
    /// Run a one-off execution in a transient sandbox.
    Exec {
        #[arg(long)]
        runtime: SandboxRuntime,
        #[arg(long)]
        command: String,
    },
}

impl SandboxCommand {
    pub async fn execute(&self) -> Result<()> {
        let manager = SandboxManager::default();
        match self {
            SandboxCommand::Create {
                project,
                runtime,
                memory_mb,
            } => {
                let sandbox_id = manager
                    .create_sandbox(SandboxConfig {
                        project: project.clone(),
                        runtime: *runtime,
                        limits: ResourceLimits {
                            memory_mb: *memory_mb,
                            ..ResourceLimits::default()
                        },
                    })
                    .await?;
                println!("sandbox created: {sandbox_id}");
            }
            SandboxCommand::Exec { runtime, command } => {
                let sandbox_id = manager
                    .create_sandbox(SandboxConfig {
                        project: "transient".to_string(),
                        runtime: *runtime,
                        limits: ResourceLimits::default(),
                    })
                    .await?;
                manager.start_sandbox(sandbox_id).await?;
                let exec_id = manager
                    .exec_in_sandbox(sandbox_id, command.clone())
                    .await
                    .with_context(|| format!("failed to exec command in sandbox {sandbox_id}"))?;
                println!("executed command with audit id: {exec_id}");
                manager.stop_sandbox(sandbox_id).await?;
            }
        }
        Ok(())
    }
}

impl std::str::FromStr for SandboxRuntime {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "python" => Ok(SandboxRuntime::Python),
            "node" | "nodejs" => Ok(SandboxRuntime::Node),
            "rust" => Ok(SandboxRuntime::Rust),
            other => Err(anyhow::anyhow!("unknown runtime: {other}")),
        }
    }
}

impl clap::ValueEnum for SandboxRuntime {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Python, Self::Node, Self::Rust]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        match self {
            SandboxRuntime::Python => Some(clap::builder::PossibleValue::new("python")),
            SandboxRuntime::Node => Some(clap::builder::PossibleValue::new("node")),
            SandboxRuntime::Rust => Some(clap::builder::PossibleValue::new("rust")),
        }
    }
}

// Clap requires ValueEnum implementations to convert from strings. We provide Display for better UX.
impl std::fmt::Display for SandboxRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SandboxRuntime::Python => write!(f, "python"),
            SandboxRuntime::Node => write!(f, "node"),
            SandboxRuntime::Rust => write!(f, "rust"),
        }
    }
}

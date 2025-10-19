use anyhow::Result;
use clap::{Parser, Subcommand};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::io::{self, AsyncWriteExt};

#[derive(Parser, Debug)]
#[command(name = "bkg", about = "BKG CLI for interacting with the CAVE API")]
struct Cli {
    /// Base URL of the BKG API server
    #[arg(long, env = "BKG_API_BASE", default_value = "http://localhost:8080")]
    server: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Sandbox lifecycle commands
    #[command(subcommand)]
    Sandboxes(SandboxCommands),
    /// Display health information
    Health,
    /// Fetch raw Prometheus metrics
    Metrics,
}

#[derive(Subcommand, Debug)]
enum SandboxCommands {
    /// Create a new sandbox
    Create {
        #[arg(long)]
        project: String,
        #[arg(long, default_value = "wasm_quick")]
        runtime: String,
        #[arg(long)]
        cpu_cores: Option<u32>,
        #[arg(long)]
        memory_mb: Option<u32>,
        #[arg(long)]
        disk_mb: Option<u32>,
    },
    /// List all sandboxes
    List,
    /// Start a sandbox
    Start { id: String },
    /// Stop a sandbox
    Stop { id: String },
    /// Execute a command in a sandbox
    Exec { id: String, command: String },
    /// Show sandbox status
    Status { id: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = Client::new();

    match cli.command {
        Commands::Sandboxes(cmd) => handle_sandbox_command(&client, &cli.server, cmd).await?,
        Commands::Health => {
            let url = format!("{}/healthz", cli.server);
            let status: serde_json::Value = client.get(url).send().await?.json().await?;
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        Commands::Metrics => {
            let url = format!("{}/metrics", cli.server);
            let response = client.get(url).send().await?;
            let body = response.bytes().await?;
            io::stdout().write_all(&body).await?;
        }
    }

    Ok(())
}

async fn handle_sandbox_command(
    client: &Client,
    server: &str,
    command: SandboxCommands,
) -> Result<()> {
    match command {
        SandboxCommands::Create {
            project,
            runtime,
            cpu_cores,
            memory_mb,
            disk_mb,
        } => {
            let url = format!("{}/api/v1/sandboxes", server);
            let payload = CreateSandboxRequest {
                project,
                runtime,
                limits: Some(SandboxLimitsRequest {
                    cpu_cores,
                    memory_mb,
                    disk_mb,
                }),
            };
            let response: CreateSandboxResponse = client
                .post(url)
                .json(&payload)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            println!("Sandbox created: {}", response.sandbox.id);
            println!("Status: {:?}", response.sandbox.state);
        }
        SandboxCommands::List => {
            let url = format!("{}/api/v1/sandboxes", server);
            let sandboxes: Vec<SandboxStatus> = client.get(url).send().await?.json().await?;
            if sandboxes.is_empty() {
                println!("No sandboxes found");
            } else {
                for sandbox in sandboxes {
                    println!("{} [{}] - {:?}", sandbox.id, sandbox.project, sandbox.state);
                }
            }
        }
        SandboxCommands::Start { id } => {
            let url = format!("{}/api/v1/sandboxes/{}/start", server, id);
            let status: SandboxStatus = client.post(url).send().await?.json().await?;
            println!("Sandbox {} state: {:?}", status.id, status.state);
        }
        SandboxCommands::Stop { id } => {
            let url = format!("{}/api/v1/sandboxes/{}/stop", server, id);
            let status: SandboxStatus = client.post(url).send().await?.json().await?;
            println!("Sandbox {} state: {:?}", status.id, status.state);
        }
        SandboxCommands::Exec { id, command } => {
            let url = format!("{}/api/v1/sandboxes/{}/exec", server, id);
            let payload = ExecRequest { command };
            let execution: SandboxExecution = client
                .post(url)
                .json(&payload)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            println!("Stdout: {}", execution.stdout);
            if !execution.stderr.is_empty() {
                println!("Stderr: {}", execution.stderr);
            }
            println!("Exit code: {}", execution.exit_code);
        }
        SandboxCommands::Status { id } => {
            let url = format!("{}/api/v1/sandboxes/{}/status", server, id);
            let status: SandboxStatus = client.get(url).send().await?.json().await?;
            println!("Sandbox {} state: {:?}", status.id, status.state);
        }
    }

    Ok(())
}

#[derive(Debug, Serialize)]
struct CreateSandboxRequest {
    project: String,
    runtime: String,
    limits: Option<SandboxLimitsRequest>,
}

#[derive(Debug, Serialize)]
struct SandboxLimitsRequest {
    cpu_cores: Option<u32>,
    memory_mb: Option<u32>,
    disk_mb: Option<u32>,
}

#[derive(Debug, Serialize)]
struct ExecRequest {
    command: String,
}

#[derive(Debug, Deserialize)]
struct CreateSandboxResponse {
    sandbox: SandboxStatus,
}

#[derive(Debug, Deserialize)]
struct SandboxStatus {
    id: String,
    state: SandboxState,
    runtime: String,
    project: String,
    limits: SandboxLimits,
    last_execution: Option<SandboxExecution>,
}

#[derive(Debug, Deserialize)]
struct SandboxLimits {
    cpu_cores: u32,
    memory_mb: u32,
    disk_mb: u32,
}

#[derive(Debug, Deserialize)]
enum SandboxState {
    Created,
    Running,
    Stopped,
}

#[derive(Debug, Deserialize)]
struct SandboxExecution {
    stdout: String,
    stderr: String,
    exit_code: i32,
    timestamp: String,
}

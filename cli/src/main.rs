//! Canopus CLI binary
//!
//! Command-line interface for interacting with the Canopus daemon.

#![allow(unused_crate_dependencies)]

use canopus_core::ClientConfig;
use clap::{Parser, Subcommand};
use cli::Client;
use ipc::uds_client::JsonRpcClient;
use schema::ServiceEvent;
use tracing::error;

#[derive(Parser)]
#[command(name = "canopus")]
#[command(about = "A CLI tool to manage the Canopus daemon")]
#[command(version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Daemon host
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Daemon port
    #[arg(long, default_value_t = 8080)]
    port: u16,
}

#[derive(Subcommand)]
enum Commands {
    /// Get daemon status
    Status,
    /// Start the daemon
    Start,
    /// Stop the daemon
    Stop,
    /// Restart the daemon
    Restart,
    /// Send a custom command to the daemon
    Custom {
        /// The custom command to send
        command: String,
    },
    /// Local UDS control plane
    Local {
        #[command(subcommand)]
        cmd: LocalCmd,
        /// UDS socket path
        #[arg(long, default_value = "/tmp/canopus.sock")]
        socket: String,
        /// Optional bearer token
        #[arg(long)]
        token: Option<String>,
    },
}

#[derive(Subcommand)]
enum LocalCmd {
    /// Get daemon version via UDS
    Version,
    /// List services
    List,
    /// Start a service
    Start { service_id: String },
    /// Stop a service
    Stop { service_id: String },
    /// Restart a service
    Restart { service_id: String },
    /// Health check a service
    Health { service_id: String },
    /// Tail service logs (prints events)
    TailLogs { service_id: String },
}

#[tokio::main]
async fn main() -> canopus_core::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let config = ClientConfig {
        daemon_host: cli.host,
        daemon_port: cli.port,
        timeout_seconds: 30,
    };

    let client = Client::new(config);

    let result = match &cli.command {
        Commands::Status => client.status().await.map_err(cli_to_core),
        Commands::Start => client.start().await.map_err(cli_to_core),
        Commands::Stop => client.stop().await.map_err(cli_to_core),
        Commands::Restart => client.restart().await.map_err(cli_to_core),
        Commands::Custom { command } => client.custom(command).await.map_err(cli_to_core),
        Commands::Local { cmd, socket, token } => {
            let uds = JsonRpcClient::new(socket, token.clone());
            match cmd {
                LocalCmd::Version => {
                    let v = uds.version().await.map_err(anyhow_to_core)?;
                    println!("Version: {}", v);
                    Ok(())
                }
                LocalCmd::List => {
                    let services = uds.list().await.map_err(anyhow_to_core)?;
                    if services.is_empty() {
                        println!("No services");
                    } else {
                        for s in services {
                            println!("{}\t{}\t{:?}", s.id, s.name, s.state);
                        }
                    }
                    Ok(())
                }
                LocalCmd::Start { service_id } => {
                    uds.start(service_id).await.map_err(anyhow_to_core)
                }
                LocalCmd::Stop { service_id } => uds.stop(service_id).await.map_err(anyhow_to_core),
                LocalCmd::Restart { service_id } => {
                    uds.restart(service_id).await.map_err(anyhow_to_core)
                }
                LocalCmd::Health { service_id } => {
                    let healthy = uds.health_check(service_id).await.map_err(anyhow_to_core)?;
                    println!(
                        "{}: {}",
                        service_id,
                        if healthy { "healthy" } else { "unhealthy" }
                    );
                    Ok(())
                }
                LocalCmd::TailLogs { service_id } => {
                    let mut rx = uds
                        .tail_logs(service_id, None)
                        .await
                        .map_err(anyhow_to_core)?;
                    while let Some(evt) = rx.recv().await {
                        print_event(&evt);
                    }
                    Ok(())
                }
            }
        }
    };

    if let Err(e) = result {
        error!("Command failed: {}", e);
        std::process::exit(1);
    }

    Ok(())
}

fn anyhow_to_core(e: ipc::IpcError) -> canopus_core::CoreError {
    canopus_core::CoreError::ServiceError(e.to_string())
}

fn cli_to_core(e: cli::CliError) -> canopus_core::CoreError {
    canopus_core::CoreError::ServiceError(e.to_string())
}

fn print_event(evt: &ServiceEvent) {
    match evt {
        ServiceEvent::LogOutput {
            service_id,
            stream,
            content,
            timestamp,
        } => {
            println!(
                "{} [{}] {}: {}",
                timestamp,
                match stream {
                    schema::LogStream::Stdout => "STDOUT",
                    schema::LogStream::Stderr => "STDERR",
                },
                service_id,
                content
            );
        }
        other => {
            println!("EVENT: {:?}", other);
        }
    }
}

//! Canopus CLI binary
//!
//! Command-line interface for interacting with the Canopus daemon.

#![allow(unused_crate_dependencies)]

use canopus_core::config::{load_services_from_toml_path, load_simple_services_from_toml_path};
use canopus_core::ClientConfig;
use clap::{Parser, Subcommand};
use cli::Client;
use ipc::uds_client::JsonRpcClient;
use schema::ServiceEvent;
use std::path::PathBuf;
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
    #[arg(long, default_value_t = 49384)]
    port: u16,
}

#[derive(Subcommand)]
enum Commands {
    /// Get daemon status
    Status,
    /// Print daemon version
    Version,
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
    /// Manage services via local UDS control plane
    Services {
        #[command(subcommand)]
        cmd: ServicesCmd,
        /// UDS socket path
        #[arg(long, default_value = "/tmp/canopus.sock")]
        socket: String,
        /// Optional bearer token
        #[arg(long)]
        token: Option<String>,
    },
}

#[derive(Subcommand)]
enum ServicesCmd {
    /// List services
    List,
    /// Show status for a service
    Status { service_id: String },
    /// Start services, either a single service by ID or using a config file
    Start {
        /// Path to a TOML config. If it contains per-service tables, they are treated as runtime overrides (hostname/port) and will stop+delete unlisted services. If it contains a services array, those service IDs will be started if known to the daemon.
        #[arg(long, value_name = "FILE", conflicts_with_all = ["service_id", "port", "hostname"])]
        config: Option<PathBuf>,
        /// Service ID to start (mutually exclusive with --config)
        #[arg(required_unless_present = "config", value_name = "SERVICE_ID")]
        service_id: Option<String>,
        /// Preferred port to run the service on (only with SERVICE_ID)
        #[arg(long)]
        port: Option<u16>,
        /// Hostname alias to bind to this service (only with SERVICE_ID)
        #[arg(long)]
        hostname: Option<String>,
    },
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
        Commands::Version => client.version().await.map_err(cli_to_core),
        Commands::Start => client.start().await.map_err(cli_to_core),
        Commands::Stop => client.stop().await.map_err(cli_to_core),
        Commands::Restart => client.restart().await.map_err(cli_to_core),
        Commands::Custom { command } => client.custom(command).await.map_err(cli_to_core),
        Commands::Services { cmd, socket, token } => {
            let uds = JsonRpcClient::new(socket, token.clone());
            match cmd {
                ServicesCmd::List => {
                    let services = uds.list().await.map_err(anyhow_to_core)?;
                    if services.is_empty() {
                        println!("No services");
                    } else {
                        for s in services {
                            let mut extras: Vec<String> = Vec::new();
                            if let Some(pid) = s.pid {
                                extras.push(format!("PID:{}", pid));
                            }
                            if let Some(port) = s.port {
                                extras.push(format!("PORT:{}", port));
                            }
                            if let Some(hn) = &s.hostname {
                                extras.push(format!("HOST:{}", hn));
                            }
                            if extras.is_empty() {
                                println!("{}\t{}\t{:?}", s.id, s.name, s.state);
                            } else {
                                println!(
                                    "{}\t{}\t{:?}\t{}",
                                    s.id,
                                    s.name,
                                    s.state,
                                    extras.join(" ")
                                );
                            }
                        }
                    }
                    Ok(())
                }
                ServicesCmd::Status { service_id } => {
                    let d = uds.status(service_id).await.map_err(anyhow_to_core)?;
                    println!("Service Status:");
                    println!("  ID: {}", d.id);
                    println!("  Name: {}", d.name);
                    println!("  State: {:?}", d.state);
                    if let Some(pid) = d.pid {
                        println!("  PID: {}", pid);
                    }
                    if let Some(port) = d.port {
                        println!("  Port: {}", port);
                    }
                    if let Some(hn) = d.hostname {
                        println!("  Hostname: {}", hn);
                    }
                    Ok(())
                }
                ServicesCmd::Start {
                    config,
                    service_id,
                    port,
                    hostname,
                } => {
                    if let Some(cfg_path) = config {
                        // Try simple runtime config first
                        match load_simple_services_from_toml_path(cfg_path) {
                            Ok(simple) => {
                                // Apply runtime config semantics using the provided UDS socket
                                // Gather current services from daemon
                                let current = uds.list().await.map_err(anyhow_to_core)?;
                                let current_ids: std::collections::HashSet<String> =
                                    current.iter().map(|s| s.id.clone()).collect();
                                let desired_ids: std::collections::HashSet<String> =
                                    simple.services.keys().cloned().collect();

                                // Stop and delete services not in desired set
                                for s in &current {
                                    if !desired_ids.contains(&s.id) {
                                        let _ = uds.stop(&s.id).await;
                                        let _ = uds.delete_meta(&s.id).await;
                                        println!(
                                            "Removed service '{}' from DB (not in config)",
                                            s.id
                                        );
                                    }
                                }

                                // Start desired services if idle
                                for (id, cfg) in &simple.services {
                                    if !current_ids.contains(id) {
                                        println!(
                                            "Warning: service '{}' not found in daemon; ensure it is defined in daemon's services config",
                                            id
                                        );
                                        continue;
                                    }
                                    let detail = uds.status(id).await.map_err(anyhow_to_core)?;
                                    if detail.state != schema::ServiceState::Idle {
                                        println!(
                                            "Skipping '{}': already running ({:?})",
                                            id, detail.state
                                        );
                                        continue;
                                    }
                                    uds.start(id, cfg.port, cfg.hostname.as_deref())
                                        .await
                                        .map_err(anyhow_to_core)?;
                                    println!(
                                        "Started '{}'{}{}",
                                        id,
                                        cfg.port
                                            .map(|p| format!(" on port {}", p))
                                            .unwrap_or_default(),
                                        cfg.hostname
                                            .as_ref()
                                            .map(|h| format!(" with host {}", h))
                                            .unwrap_or_default()
                                    );
                                }
                                Ok(())
                            }
                            Err(_) => {
                                // Not a simple runtime; try full services file and start listed IDs
                                match load_services_from_toml_path(cfg_path) {
                                    Ok(services_file) => {
                                        let services = services_file.services;
                                        if services.is_empty() {
                                            println!("No services in config");
                                            return Ok(());
                                        }
                                        for spec in services {
                                            let id = spec.id;
                                            // Check if known
                                            match uds.status(&id).await {
                                                Ok(detail) => {
                                                    if detail.state != schema::ServiceState::Idle {
                                                        println!(
                                                            "Skipping '{}': already running ({:?})",
                                                            id, detail.state
                                                        );
                                                        continue;
                                                    }
                                                    uds.start(&id, None, None)
                                                        .await
                                                        .map_err(anyhow_to_core)?;
                                                    println!("Started '{}'", id);
                                                }
                                                Err(_) => {
                                                    println!(
                                                        "Warning: service '{}' not found in daemon; ensure daemon loaded matching services config",
                                                        id
                                                    );
                                                }
                                            }
                                        }
                                        Ok(())
                                    }
                                    Err(e) => Err(anyhow_to_core(ipc::IpcError::ProtocolError(
                                        format!("failed to parse config: {}", e),
                                    ))),
                                }
                            }
                        }
                    } else {
                        // Single service start path
                        let id = service_id
                            .as_deref()
                            .expect("SERVICE_ID required unless --config is provided");
                        uds.start(id, *port, hostname.as_deref())
                            .await
                            .map_err(anyhow_to_core)
                    }
                }
                ServicesCmd::Stop { service_id } => {
                    uds.stop(service_id).await.map_err(anyhow_to_core)
                }
                ServicesCmd::Restart { service_id } => {
                    uds.restart(service_id).await.map_err(anyhow_to_core)
                }
                ServicesCmd::Health { service_id } => {
                    let healthy = uds.health_check(service_id).await.map_err(anyhow_to_core)?;
                    println!(
                        "{}: {}",
                        service_id,
                        if healthy { "healthy" } else { "unhealthy" }
                    );
                    Ok(())
                }
                ServicesCmd::TailLogs { service_id } => {
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

//! Canopus CLI binary
//!
//! Command-line interface for interacting with the Canopus daemon.

#![allow(unused_crate_dependencies)]

use canopus_core::config::{load_services_from_toml_path, load_simple_services_from_toml_path};
use canopus_core::ClientConfig;
use canopus_inbox::{truncate, InboxFilter, InboxStatus, InboxStore, NewInboxItem, SourceAgent, SqliteStore};
use clap::{Parser, Subcommand, ValueEnum};
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
    /// Manage AI agent inbox notifications
    Inbox {
        #[command(subcommand)]
        cmd: InboxCmd,
    },
}

/// CLI-friendly source agent enum
#[derive(Debug, Clone, Copy, ValueEnum)]
enum SourceAgentArg {
    /// Claude Code by Anthropic
    ClaudeCode,
    /// `OpenAI` Codex CLI
    Codex,
    /// Windsurf IDE
    Windsurf,
    /// `OpenCode` AI CLI
    OpenCode,
    /// Other/unknown agent
    Other,
}

impl From<SourceAgentArg> for SourceAgent {
    fn from(arg: SourceAgentArg) -> Self {
        match arg {
            SourceAgentArg::ClaudeCode => Self::ClaudeCode,
            SourceAgentArg::Codex => Self::Codex,
            SourceAgentArg::Windsurf => Self::Windsurf,
            SourceAgentArg::OpenCode => Self::OpenCode,
            SourceAgentArg::Other => Self::Other,
        }
    }
}

/// CLI-friendly inbox status enum
#[derive(Debug, Clone, Copy, ValueEnum)]
enum InboxStatusArg {
    /// Unread items
    Unread,
    /// Read items
    Read,
    /// Dismissed items
    Dismissed,
}

impl From<InboxStatusArg> for InboxStatus {
    fn from(arg: InboxStatusArg) -> Self {
        match arg {
            InboxStatusArg::Unread => Self::Unread,
            InboxStatusArg::Read => Self::Read,
            InboxStatusArg::Dismissed => Self::Dismissed,
        }
    }
}

#[derive(Subcommand)]
enum InboxCmd {
    /// List inbox items
    List {
        /// Filter by status
        #[arg(long, value_enum)]
        status: Option<InboxStatusArg>,
        /// Filter by source agent
        #[arg(long, value_enum)]
        agent: Option<SourceAgentArg>,
        /// Filter by project name (partial match)
        #[arg(long)]
        project: Option<String>,
        /// Maximum number of items to show
        #[arg(long, default_value = "20")]
        limit: u32,
        /// Show only count
        #[arg(long)]
        count: bool,
    },
    /// View a specific inbox item (marks as read)
    View {
        /// Item ID
        id: String,
    },
    /// Add a new inbox item (used by agent hooks)
    Add {
        /// Project name
        #[arg(long)]
        project: String,
        /// Status summary
        #[arg(long)]
        status: String,
        /// Action required
        #[arg(long)]
        action: String,
        /// Source agent
        #[arg(long, value_enum)]
        agent: SourceAgentArg,
    },
    /// Mark items as read
    Read {
        /// Item ID(s) to mark as read
        ids: Vec<String>,
    },
    /// Dismiss items
    Dismiss {
        /// Item ID(s) to dismiss
        ids: Vec<String>,
    },
    /// Check for new items and show desktop notification
    Check {
        /// Suppress desktop notification
        #[arg(long)]
        quiet: bool,
    },
    /// Clean up old items (older than 7 days)
    Cleanup {
        /// Number of days to keep (default: 7)
        #[arg(long, default_value = "7")]
        days: i64,
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
        /// Preferred port to run the service on (only with `SERVICE_ID`)
        #[arg(long)]
        port: Option<u16>,
        /// Hostname alias to bind to this service (only with `SERVICE_ID`)
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
#[allow(clippy::too_many_lines)]
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
        Commands::Status => client.status().await.map_err(|e| cli_to_core(&e)),
        Commands::Version => client.version().await.map_err(|e| cli_to_core(&e)),
        Commands::Start => client.start().await.map_err(|e| cli_to_core(&e)),
        Commands::Stop => client.stop().await.map_err(|e| cli_to_core(&e)),
        Commands::Restart => client.restart().await.map_err(|e| cli_to_core(&e)),
        Commands::Custom { command } => client.custom(command).await.map_err(|e| cli_to_core(&e)),
        Commands::Services { cmd, socket, token } => {
            let uds = JsonRpcClient::new(socket, token.clone());
            match cmd {
                ServicesCmd::List => {
                    let services = uds.list().await.map_err(|e| anyhow_to_core(&e))?;
                    if services.is_empty() {
                        println!("No services");
                    } else {
                        for s in services {
                            let mut extras: Vec<String> = Vec::new();
                            if let Some(pid) = s.pid {
                                extras.push(format!("PID:{pid}"));
                            }
                            if let Some(port) = s.port {
                                extras.push(format!("PORT:{port}"));
                            }
                            if let Some(hn) = &s.hostname {
                                extras.push(format!("HOST:{hn}"));
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
                    let d = uds.status(service_id).await.map_err(|e| anyhow_to_core(&e))?;
                    println!("Service Status:");
                    println!("  ID: {}", d.id);
                    println!("  Name: {}", d.name);
                    println!("  State: {:?}", d.state);
                    if let Some(pid) = d.pid {
                        println!("  PID: {pid}");
                    }
                    if let Some(port) = d.port {
                        println!("  Port: {port}");
                    }
                    if let Some(hn) = d.hostname {
                        println!("  Hostname: {hn}");
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
                                let current = uds.list().await.map_err(|e| anyhow_to_core(&e))?;
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
                                            "Warning: service '{id}' not found in daemon; ensure it is defined in daemon's services config"
                                        );
                                        continue;
                                    }
                                    let detail = uds.status(id).await.map_err(|e| anyhow_to_core(&e))?;
                                    if detail.state != schema::ServiceState::Idle {
                                        println!(
                                            "Skipping '{}': already running ({:?})",
                                            id, detail.state
                                        );
                                        continue;
                                    }
                                    uds.start(id, cfg.port, cfg.hostname.as_deref())
                                        .await
                                        .map_err(|e| anyhow_to_core(&e))?;
                                    println!(
                                        "Started '{}'{}{}",
                                        id,
                                        cfg.port
                                            .map(|p| format!(" on port {p}"))
                                            .unwrap_or_default(),
                                        cfg.hostname
                                            .as_ref()
                                            .map(|h| format!(" with host {h}"))
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
                                                        .map_err(|e| anyhow_to_core(&e))?;
                                                    println!("Started '{id}'");
                                                }
                                                Err(_) => {
                                                    println!(
                                                        "Warning: service '{id}' not found in daemon; ensure daemon loaded matching services config"
                                                    );
                                                }
                                            }
                                        }
                                        Ok(())
                                    }
                                    Err(e) => Err(anyhow_to_core(&ipc::IpcError::ProtocolError(
                                        format!("failed to parse config: {e}"),
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
                            .map_err(|e| anyhow_to_core(&e))
                    }
                }
                ServicesCmd::Stop { service_id } => {
                    uds.stop(service_id).await.map_err(|e| anyhow_to_core(&e))
                }
                ServicesCmd::Restart { service_id } => {
                    uds.restart(service_id).await.map_err(|e| anyhow_to_core(&e))
                }
                ServicesCmd::Health { service_id } => {
                    let healthy = uds.health_check(service_id).await.map_err(|e| anyhow_to_core(&e))?;
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
                        .map_err(|e| anyhow_to_core(&e))?;
                    while let Some(evt) = rx.recv().await {
                        print_event(&evt);
                    }
                    Ok(())
                }
            }
        }
        Commands::Inbox { cmd } => handle_inbox_command(cmd).await,
    };

    if let Err(e) = result {
        error!("Command failed: {}", e);
        std::process::exit(1);
    }

    Ok(())
}

fn anyhow_to_core(e: &ipc::IpcError) -> canopus_core::CoreError {
    canopus_core::CoreError::ServiceError(e.to_string())
}

fn cli_to_core(e: &cli::CliError) -> canopus_core::CoreError {
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
            println!("EVENT: {other:?}");
        }
    }
}

/// Handle inbox subcommands.
#[allow(clippy::too_many_lines)]
async fn handle_inbox_command(cmd: &InboxCmd) -> canopus_core::Result<()> {
    let store = SqliteStore::open_default().map_err(|e| inbox_to_core(&e))?;

    match cmd {
        InboxCmd::List {
            status,
            agent,
            project,
            limit,
            count,
        } => {
            let filter = InboxFilter {
                status: status.map(Into::into),
                source_agent: agent.map(Into::into),
                project: project.clone(),
                limit: Some(*limit),
            };

            if *count {
                let n = store.count(filter).await.map_err(|e| inbox_to_core(&e))?;
                println!("{n}");
            } else {
                let items = store.list(filter).await.map_err(|e| inbox_to_core(&e))?;
                if items.is_empty() {
                    println!("No inbox items");
                } else {
                    println!(
                        "{:<36} {:<15} {:<12} {:<10} ACTION",
                        "ID", "PROJECT", "AGENT", "AGE"
                    );
                    println!("{}", "-".repeat(100));
                    for item in items {
                        let status_marker = match item.status {
                            InboxStatus::Unread => "*",
                            InboxStatus::Read => " ",
                            InboxStatus::Dismissed => "x",
                        };
                        println!(
                            "{}{:<35} {:<15} {:<12} {:<10} {}",
                            status_marker,
                            truncate(&item.id, 35),
                            truncate(&item.project_name, 15),
                            item.source_agent.display_name(),
                            item.age_display(),
                            truncate(&item.action_required, 40)
                        );
                    }
                }
            }
            Ok(())
        }
        InboxCmd::View { id } => {
            let item = store
                .get(id)
                .await
                .map_err(|e| inbox_to_core(&e))?
                .ok_or_else(|| {
                canopus_core::CoreError::ServiceError(format!("Item not found: {id}"))
            })?;

            // Mark as read
            store.mark_read(id).await.map_err(|e| inbox_to_core(&e))?;

            println!("Inbox Item: {}", item.id);
            println!("{}", "=".repeat(60));
            println!("Project:    {}", item.project_name);
            println!("Agent:      {}", item.source_agent.display_name());
            println!("Status:     {}", item.status);
            println!("Created:    {} ({})", item.created_at, item.age_display());
            println!();
            println!("Status Summary:");
            println!("  {}", item.status_summary);
            println!();
            println!("Action Required:");
            println!("  {}", item.action_required);

            if let Some(details) = &item.details {
                println!();
                println!("Details:");
                println!(
                    "  {}",
                    serde_json::to_string_pretty(details).unwrap_or_default()
                );
            }
            Ok(())
        }
        InboxCmd::Add {
            project,
            status,
            action,
            agent,
        } => {
            let new_item = NewInboxItem::new(project, status, action, (*agent).into());
            let item = store.insert(new_item).await.map_err(|e| inbox_to_core(&e))?;

            // Send desktop notification
            if let Err(e) = canopus_inbox::notify::send_notification(&item) {
                tracing::warn!("Failed to send notification: {}", e);
            }

            // Mark as notified
            if let Err(e) = store.mark_notified(&item.id).await {
                tracing::warn!(
                    "Failed to mark item {} as notified: {}. User may receive duplicate notifications.",
                    item.id, e
                );
            }

            println!("Added inbox item: {}", item.id);
            Ok(())
        }
        InboxCmd::Read { ids } => {
            for id in ids {
                store.mark_read(id).await.map_err(|e| inbox_to_core(&e))?;
                println!("Marked as read: {id}");
            }
            Ok(())
        }
        InboxCmd::Dismiss { ids } => {
            for id in ids {
                store.dismiss(id).await.map_err(|e| inbox_to_core(&e))?;
                println!("Dismissed: {id}");
            }
            Ok(())
        }
        InboxCmd::Check { quiet } => {
            let filter = InboxFilter {
                status: Some(InboxStatus::Unread),
                ..Default::default()
            };
            let items = store.list(filter).await.map_err(|e| inbox_to_core(&e))?;
            let count = items.len();

            if count == 0 {
                if !*quiet {
                    println!("No new inbox items");
                }
                return Ok(());
            }

            println!("{count} new inbox item(s)");
            for item in items.iter().take(5) {
                println!(
                    "  * [{}] {}: {}",
                    item.source_agent.display_name(),
                    item.project_name,
                    truncate(&item.action_required, 50)
                );
            }
            if count > 5 {
                println!("  ... and {} more", count.saturating_sub(5));
            }

            // Send desktop notification
            if !*quiet {
                if let Err(e) = canopus_inbox::notify::send_summary_notification(count, &items) {
                    tracing::warn!("Failed to send notification: {}", e);
                }
            }

            Ok(())
        }
        InboxCmd::Cleanup { days } => {
            let deleted = store
                .cleanup_older_than(*days)
                .await
                .map_err(|e| inbox_to_core(&e))?;
            println!("Cleaned up {deleted} old inbox items");
            Ok(())
        }
    }
}

fn inbox_to_core(e: &canopus_inbox::InboxError) -> canopus_core::CoreError {
    canopus_core::CoreError::ServiceError(e.to_string())
}

//! CLI library for the Canopus project

#![allow(unused_crate_dependencies)]

pub mod error;

pub use error::{CliError, Result};
use ipc::IpcClient;
use schema::{ClientConfig, Message, Response};
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tokio::time::{sleep, Duration};
use tracing::error;

use canopus_core::config::{load_simple_services_from_toml_path, SimpleServicesFile};
use ipc::uds_client::JsonRpcClient;

/// CLI client for communicating with the daemon
#[derive(Debug)]
pub struct Client {
    #[allow(dead_code)]
    config: ClientConfig,
    ipc_client: IpcClient,
}

impl Client {
    /// Create a new CLI client
    pub fn new(config: ClientConfig) -> Self {
        let ipc_client = IpcClient::new(&config.daemon_host, config.daemon_port);
        Self { config, ipc_client }
    }

    /// Start the daemon and, if a config is provided, synchronize services to match it
    pub async fn start_with_config(&self, config: Option<PathBuf>) -> Result<()> {
        // Ensure daemon is running (reuses existing logic)
        self.start().await?;

        // If no config provided, nothing more to do
        let Some(cfg_path) = config else {
            return Ok(());
        };

        // Parse simple config: tables per service id with optional hostname/port
        let simple: SimpleServicesFile = load_simple_services_from_toml_path(&cfg_path)
            .map_err(|e| CliError::DaemonError(format!("config error: {}", e)))?;

        // Connect to local UDS control plane
        let socket =
            std::env::var("CANOPUS_IPC_SOCKET").unwrap_or_else(|_| "/tmp/canopus.sock".to_string());
        let uds = JsonRpcClient::new(socket, std::env::var("CANOPUS_IPC_TOKEN").ok());

        // Gather current services from daemon
        let current = uds.list().await.map_err(CliError::IpcError)?;
        let current_ids: HashSet<String> = current.iter().map(|s| s.id.clone()).collect();

        // Compute desired set from config
        let desired_ids: HashSet<String> = simple.services.keys().cloned().collect();

        // Stop and delete services not in desired set
        for s in &current {
            if !desired_ids.contains(&s.id) {
                // Best-effort stop
                let _ = uds.stop(&s.id).await;
                // Remove metadata row from SQLite
                let _ = uds.delete_meta(&s.id).await;
                println!("Removed service '{}' from DB (not in config)", s.id);
            }
        }

        // For each desired service, start if idle; skip if already running
        for (id, cfg) in &simple.services {
            if !current_ids.contains(id) {
                // Unknown service id to the daemon; skip with a warning
                println!(
                    "Warning: service '{}' not found in daemon; ensure it is defined in daemon's services config",
                    id
                );
                continue;
            }

            let detail = uds.status(id).await.map_err(CliError::IpcError)?;
            if detail.state != schema::ServiceState::Idle {
                println!("Skipping '{}': already running ({:?})", id, detail.state);
                continue;
            }

            uds.start(id, cfg.port, cfg.hostname.as_deref())
                .await
                .map_err(CliError::IpcError)?;
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

    /// Connect to the daemon and send a message
    pub async fn send_message(&self, message: Message) -> Result<Response> {
        self.ipc_client
            .send_message(&message)
            .await
            .map_err(CliError::IpcError)
    }

    /// Get daemon status
    pub async fn status(&self) -> Result<()> {
        match self.send_message(Message::Status).await {
            Ok(Response::Status {
                running,
                uptime_seconds,
                pid,
                version,
            }) => {
                println!("Daemon Status:");
                println!("  Running: {}", running);
                println!("  Uptime: {} seconds", uptime_seconds);
                println!("  PID: {}", pid);
                if let Some(v) = version {
                    println!("  Version: {}", v);
                }
            }
            Ok(Response::Error { message, code }) => {
                error!("Error getting status: {}", message);
                if let Some(c) = code {
                    error!("Error code: {}", c);
                }
                return Err(CliError::DaemonError(message));
            }
            Ok(_) => {
                let err = "Unexpected response type".to_string();
                error!("{}", err);
                return Err(CliError::DaemonError(err));
            }
            Err(CliError::IpcError(ipc_err)) => {
                // If the daemon isn't running, report a friendly status instead of a raw connection error
                if let ipc::IpcError::ConnectionFailed(_) = ipc_err {
                    println!("Daemon Status:");
                    println!("  Running: false");
                    println!("  Note: daemon not started");
                    return Ok(());
                }
                Err(CliError::IpcError(ipc_err))?
            }
            Err(e) => return Err(e),
        }
        Ok(())
    }

    /// Start the daemon
    pub async fn start(&self) -> Result<()> {
        // First, try to send Start to a running daemon
        match self.send_message(Message::Start).await {
            Ok(resp) => match resp {
                Response::Ok { message } => {
                    println!("✓ {}", message);
                    Ok(())
                }
                Response::Error { message, code } => {
                    // Treat already running as success
                    if matches!(code.as_deref(), Some("DAEMON_ALREADY_RUNNING")) {
                        println!("✓ Daemon already running");
                        Ok(())
                    } else {
                        error!("Failed to start daemon: {}", message);
                        if let Some(c) = code {
                            error!("Error code: {}", c);
                        }
                        Err(CliError::DaemonError(message))
                    }
                }
                _ => {
                    let err = "Unexpected response type".to_string();
                    error!("{}", err);
                    Err(CliError::DaemonError(err))
                }
            },
            Err(CliError::IpcError(ipc_err)) => {
                // Connection failure: try to spawn the daemon, then wait for readiness
                if let ipc::IpcError::ConnectionFailed(_) = ipc_err {
                    println!("Daemon not running; attempting to start it...");
                    self.spawn_daemon_background()?;
                    // Wait up to 20s for the daemon to become reachable
                    self.wait_for_ready(Duration::from_secs(20)).await?;
                    // After ready, sending Start is optional; do it for symmetry
                    match self.send_message(Message::Start).await {
                        Ok(Response::Ok { message }) => {
                            println!("✓ {}", message);
                            Ok(())
                        }
                        Ok(Response::Error { message, code }) => {
                            if matches!(code.as_deref(), Some("DAEMON_ALREADY_RUNNING")) {
                                println!("✓ Daemon started and is running");
                                Ok(())
                            } else {
                                error!("Failed to start daemon: {}", message);
                                if let Some(c) = code {
                                    error!("Error code: {}", c);
                                }
                                Err(CliError::DaemonError(message))
                            }
                        }
                        Ok(_) => {
                            let err = "Unexpected response type".to_string();
                            error!("{}", err);
                            Err(CliError::DaemonError(err))
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    Err(CliError::IpcError(ipc_err))
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Stop the daemon
    pub async fn stop(&self) -> Result<()> {
        match self.send_message(Message::Stop).await? {
            Response::Ok { message } => {
                println!("✓ {}", message);
            }
            Response::Error { message, code } => {
                error!("Failed to stop daemon: {}", message);
                if let Some(c) = code {
                    error!("Error code: {}", c);
                }
                return Err(CliError::DaemonError(message));
            }
            _ => {
                let err = "Unexpected response type".to_string();
                error!("{}", err);
                return Err(CliError::DaemonError(err));
            }
        }
        Ok(())
    }

    /// Restart the daemon
    pub async fn restart(&self) -> Result<()> {
        match self.send_message(Message::Restart).await? {
            Response::Ok { message } => {
                println!("✓ {}", message);
            }
            Response::Error { message, code } => {
                error!("Failed to restart daemon: {}", message);
                if let Some(c) = code {
                    error!("Error code: {}", c);
                }
                return Err(CliError::DaemonError(message));
            }
            _ => {
                let err = "Unexpected response type".to_string();
                error!("{}", err);
                return Err(CliError::DaemonError(err));
            }
        }
        Ok(())
    }

    /// Send a custom command
    pub async fn custom(&self, command: &str) -> Result<()> {
        match self
            .send_message(Message::Custom {
                cmd: command.to_string(),
            })
            .await?
        {
            Response::Ok { message } => {
                println!("✓ {}", message);
            }
            Response::Error { message, code } => {
                error!("Command failed: {}", message);
                if let Some(c) = code {
                    error!("Error code: {}", c);
                }
                return Err(CliError::DaemonError(message));
            }
            _ => {
                let err = "Unexpected response type".to_string();
                error!("{}", err);
                return Err(CliError::DaemonError(err));
            }
        }
        Ok(())
    }
}

impl Client {
    /// Get daemon semantic version via TCP Status response
    pub async fn version(&self) -> Result<()> {
        match self.send_message(Message::Status).await? {
            Response::Status { version, .. } => match version {
                Some(v) => {
                    println!("{}", v);
                    Ok(())
                }
                None => Err(CliError::DaemonError("version unavailable".into())),
            },
            Response::Error { message, .. } => Err(CliError::DaemonError(message)),
            _ => Err(CliError::DaemonError("Unexpected response".into())),
        }
    }
}

impl Client {
    async fn wait_for_ready(&self, timeout: Duration) -> Result<()> {
        let start = std::time::Instant::now();
        let mut last_err: Option<CliError> = None;
        while start.elapsed() < timeout {
            match self.send_message(Message::Status).await {
                Ok(Response::Status { running, .. }) => {
                    if running {
                        return Ok(());
                    }
                }
                Ok(_) => { /* keep trying */ }
                Err(e) => {
                    last_err = Some(e);
                }
            }
            sleep(Duration::from_millis(200)).await;
        }
        Err(last_err
            .unwrap_or_else(|| CliError::ConnectionFailed("Timed out waiting for daemon".into())))
    }

    fn spawn_daemon_background(&self) -> Result<()> {
        // Prefer spawning the daemon binary from the same target directory as this CLI binary
        if let Ok(path) = std::env::current_exe() {
            if let Some(bin_dir) = path.parent() {
                // Replace canopus with daemon, preserving .exe suffix on Windows if any
                let file_name = if cfg!(windows) {
                    "daemon.exe"
                } else {
                    "daemon"
                };
                let daemon_path: PathBuf = bin_dir.join(file_name);
                if daemon_path.exists() {
                    let _child = Command::new(&daemon_path)
                        .arg("--host")
                        .arg(&self.config.daemon_host)
                        .arg("--port")
                        .arg(self.config.daemon_port.to_string())
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .map_err(CliError::IoError)?;
                    return Ok(());
                }
            }
        }

        // Fallback: rely on PATH to find a `daemon` binary
        let _child = Command::new("daemon")
            .arg("--host")
            .arg(&self.config.daemon_host)
            .arg("--port")
            .arg(self.config.daemon_port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(CliError::IoError)?;
        Ok(())
    }
}

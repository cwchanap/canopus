//! CLI library for the Canopus project

#![allow(unused_crate_dependencies)]

pub mod error;

pub use error::{CliError, Result};
use ipc::IpcClient;
use schema::{ClientConfig, Message, Response};
use tracing::error;
use tokio::time::{sleep, Duration};
use std::path::PathBuf;
use std::process::{Command, Stdio};

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

    /// Connect to the daemon and send a message
    pub async fn send_message(&self, message: Message) -> Result<Response> {
        self.ipc_client
            .send_message(&message)
            .await
            .map_err(CliError::IpcError)
    }

    /// Get daemon status
    pub async fn status(&self) -> Result<()> {
        match self.send_message(Message::Status).await? {
            Response::Status {
                running,
                uptime_seconds,
                version,
            } => {
                println!("Daemon Status:");
                println!("  Running: {}", running);
                println!("  Uptime: {} seconds", uptime_seconds);
                if let Some(v) = version {
                    println!("  Version: {}", v);
                }
            }
            Response::Error { message, code } => {
                error!("Error getting status: {}", message);
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

    /// Start the daemon
    pub async fn start(&self) -> Result<()> {
        // First, try to send Start to a running daemon
        match self.send_message(Message::Start).await {
            Ok(resp) => match resp {
                Response::Ok { message } => {
                    println!("✓ {}", message);
                    return Ok(());
                }
                Response::Error { message, code } => {
                    // Treat already running as success
                    if matches!(code.as_deref(), Some("DAEMON_ALREADY_RUNNING")) {
                        println!("✓ Daemon already running");
                        return Ok(());
                    }
                    error!("Failed to start daemon: {}", message);
                    if let Some(c) = code { error!("Error code: {}", c); }
                    return Err(CliError::DaemonError(message));
                }
                _ => {
                    let err = "Unexpected response type".to_string();
                    error!("{}", err);
                    return Err(CliError::DaemonError(err));
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
                                if let Some(c) = code { error!("Error code: {}", c); }
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
    async fn wait_for_ready(&self, timeout: Duration) -> Result<()> {
        let start = std::time::Instant::now();
        let mut last_err: Option<CliError> = None;
        while start.elapsed() < timeout {
            match self.send_message(Message::Status).await {
                Ok(Response::Status { running, .. }) => {
                    if running { return Ok(()); }
                }
                Ok(_) => { /* keep trying */ }
                Err(e) => { last_err = Some(e); }
            }
            sleep(Duration::from_millis(200)).await;
        }
        Err(last_err.unwrap_or_else(|| CliError::ConnectionFailed("Timed out waiting for daemon".into())))
    }

    fn spawn_daemon_background(&self) -> Result<()> {
        // Prefer spawning the daemon binary from the same target directory as this CLI binary
        if let Ok(path) = std::env::current_exe() {
            if let Some(bin_dir) = path.parent() {
                // Replace canopus with daemon, preserving .exe suffix on Windows if any
                let file_name = if cfg!(windows) { "daemon.exe" } else { "daemon" };
                let daemon_path: PathBuf = bin_dir.join(file_name);
                if daemon_path.exists() {
                    let _child = Command::new(&daemon_path)
                        .arg("--host").arg(&self.config.daemon_host)
                        .arg("--port").arg(self.config.daemon_port.to_string())
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
            .arg("--host").arg(&self.config.daemon_host)
            .arg("--port").arg(self.config.daemon_port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(CliError::IoError)?;
        Ok(())
    }
}

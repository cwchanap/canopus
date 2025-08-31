//! CLI library for the Canopus project

pub mod error;

pub use error::{CliError, Result};
use schema::{Message, Response, ClientConfig};
use ipc::IpcClient;
use tracing::error;

/// CLI client for communicating with the daemon
pub struct Client {
    config: ClientConfig,
    ipc_client: IpcClient,
}

impl Client {
    pub fn new(config: ClientConfig) -> Self {
        let ipc_client = IpcClient::new(&config.daemon_host, config.daemon_port);
        Self { config, ipc_client }
    }

    /// Connect to the daemon and send a message
    pub async fn send_message(&self, message: Message) -> Result<Response> {
        self.ipc_client.send_message(&message)
            .await
            .map_err(CliError::IpcError)
    }

    /// Get daemon status
    pub async fn status(&self) -> Result<()> {
        match self.send_message(Message::Status).await? {
            Response::Status { running, uptime_seconds, version } => {
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
        match self.send_message(Message::Start).await? {
            Response::Ok { message } => {
                println!("✓ {}", message);
            }
            Response::Error { message, code } => {
                error!("Failed to start daemon: {}", message);
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
        match self.send_message(Message::Custom { cmd: command.to_string() }).await? {
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

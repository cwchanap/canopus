use core::{Config, Message, Response, Result};
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info, error};

/// CLI client for communicating with the daemon
pub struct Client {
    config: Config,
}

impl Client {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Connect to the daemon and send a message
    pub async fn send_message(&self, message: Message) -> Result<Response> {
        let addr = format!("{}:{}", self.config.daemon_host, self.config.daemon_port);
        
        info!("Connecting to daemon at {}", addr);
        let mut stream = TcpStream::connect(&addr).await?;

        // Send message
        let message_data = serde_json::to_vec(&message)?;
        stream.write_all(&message_data).await?;

        // Read response
        let mut buffer = [0; 1024];
        let n = stream.read(&mut buffer).await?;
        
        if n == 0 {
            return Err("No response from daemon".into());
        }

        let response: Response = serde_json::from_slice(&buffer[..n])?;
        Ok(response)
    }

    /// Get daemon status
    pub async fn status(&self) -> Result<()> {
        match self.send_message(Message::Status).await? {
            Response::Status { running, uptime } => {
                println!("Daemon Status:");
                println!("  Running: {}", running);
                println!("  Uptime: {} seconds", uptime);
            }
            Response::Error(err) => {
                error!("Error getting status: {}", err);
            }
            _ => {
                error!("Unexpected response type");
            }
        }
        Ok(())
    }

    /// Start the daemon
    pub async fn start(&self) -> Result<()> {
        match self.send_message(Message::Start).await? {
            Response::Ok(msg) => {
                println!("✓ {}", msg);
            }
            Response::Error(err) => {
                error!("Failed to start daemon: {}", err);
            }
            _ => {
                error!("Unexpected response type");
            }
        }
        Ok(())
    }

    /// Stop the daemon
    pub async fn stop(&self) -> Result<()> {
        match self.send_message(Message::Stop).await? {
            Response::Ok(msg) => {
                println!("✓ {}", msg);
            }
            Response::Error(err) => {
                error!("Failed to stop daemon: {}", err);
            }
            _ => {
                error!("Unexpected response type");
            }
        }
        Ok(())
    }

    /// Restart the daemon
    pub async fn restart(&self) -> Result<()> {
        match self.send_message(Message::Restart).await? {
            Response::Ok(msg) => {
                println!("✓ {}", msg);
            }
            Response::Error(err) => {
                error!("Failed to restart daemon: {}", err);
            }
            _ => {
                error!("Unexpected response type");
            }
        }
        Ok(())
    }

    /// Send a custom command
    pub async fn custom(&self, command: &str) -> Result<()> {
        match self.send_message(Message::Custom(command.to_string())).await? {
            Response::Ok(msg) => {
                println!("✓ {}", msg);
            }
            Response::Error(err) => {
                error!("Command failed: {}", err);
            }
            _ => {
                error!("Unexpected response type");
            }
        }
        Ok(())
    }
}

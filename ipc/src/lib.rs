//! IPC (Inter-Process Communication) module
//!
//! This crate handles communication between the daemon and CLI components.

pub mod error;
pub mod server;
pub mod uds_client;

#[cfg(test)]
mod error_tests;

pub use error::{IpcError, Result};

use schema::{Message, Response};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::debug;

/// IPC client for communicating with the daemon
#[derive(Debug)]
pub struct IpcClient {
    host: String,
    port: u16,
}

impl IpcClient {
    /// Create a new IPC client
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }

    /// Connect to the daemon and send a message
    pub async fn send_message(&self, message: &Message) -> Result<Response> {
        let addr = format!("{}:{}", self.host, self.port);

        debug!("Connecting to daemon at {}", addr);
        let mut stream = TcpStream::connect(&addr)
            .await
            .map_err(|e| IpcError::ConnectionFailed(e.to_string()))?;

        // Send message
        let message_data = serde_json::to_vec(message)
            .map_err(|e| IpcError::SerializationFailed(e.to_string()))?;

        stream
            .write_all(&message_data)
            .await
            .map_err(|e| IpcError::SendFailed(e.to_string()))?;

        // Read response
        let mut buffer = [0; 4096];
        let n = stream
            .read(&mut buffer)
            .await
            .map_err(|e| IpcError::ReceiveFailed(e.to_string()))?;

        if n == 0 {
            return Err(IpcError::EmptyResponse);
        }

        let response: Response = serde_json::from_slice(&buffer[..n])
            .map_err(|e| IpcError::DeserializationFailed(e.to_string()))?;

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipc_client_creation() {
        let client = IpcClient::new("localhost", 8080);
        assert_eq!(client.host, "localhost");
        assert_eq!(client.port, 8080);
    }
}

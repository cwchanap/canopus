#![allow(unused_crate_dependencies)]
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
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tracing::debug;

/// Maximum allowed frame size for IPC messages (64KB)
/// This prevents unbounded memory growth from malicious or buggy peers
const MAX_FRAME_SIZE: usize = 64 * 1024;

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
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails, the message cannot be serialized,
    /// or the response cannot be read or deserialized.
    pub async fn send_message(&self, message: &Message) -> Result<Response> {
        let addr = format!("{}:{}", self.host, self.port);

        debug!("Connecting to daemon at {}", addr);
        let stream = TcpStream::connect(&addr)
            .await
            .map_err(|e| IpcError::ConnectionFailed(e.to_string()))?;

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        // Send message with newline delimiter
        let mut message_data = serde_json::to_vec(message)
            .map_err(|e| IpcError::SerializationFailed(e.to_string()))?;
        message_data.push(b'\n');

        writer
            .write_all(&message_data)
            .await
            .map_err(|e| IpcError::SendFailed(e.to_string()))?;

        // Read response until newline with bounded buffering
        let mut buffer = Vec::with_capacity(4096);
        loop {
            let chunk = reader
                .fill_buf()
                .await
                .map_err(|e| IpcError::ReceiveFailed(e.to_string()))?;
            if chunk.is_empty() {
                if buffer.is_empty() {
                    return Err(IpcError::EmptyResponse);
                }
                return Err(IpcError::ProtocolError(
                    "incomplete frame: connection closed before newline terminator".to_string(),
                ));
            }

            let newline_pos = chunk.iter().position(|b| *b == b'\n');
            let to_copy = newline_pos.map_or(chunk.len(), |idx| idx + 1);
            let next_len = buffer.len() + to_copy;
            if next_len > MAX_FRAME_SIZE {
                return Err(IpcError::ProtocolError(format!(
                    "Response size {next_len} exceeds maximum allowed size of {MAX_FRAME_SIZE} bytes"
                )));
            }

            buffer.extend_from_slice(&chunk[..to_copy]);
            reader.consume(to_copy);
            if newline_pos.is_some() {
                break;
            }
        }

        // Trim trailing newline/carriage return
        if matches!(buffer.last(), Some(b'\n')) {
            buffer.pop();
            if matches!(buffer.last(), Some(b'\r')) {
                buffer.pop();
            }
        }

        let response: Response = serde_json::from_slice(&buffer)
            .map_err(|e| IpcError::DeserializationFailed(e.to_string()))?;

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;

    #[test]
    fn test_ipc_client_creation() {
        let client = IpcClient::new("localhost", 8080);
        assert_eq!(client.host, "localhost");
        assert_eq!(client.port, 8080);
    }

    #[tokio::test]
    async fn test_send_message_returns_empty_response_on_immediate_eof() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut request = Vec::new();
            let _ = reader.read_until(b'\n', &mut request).await.unwrap();
        });

        let client = IpcClient::new(addr.ip().to_string(), addr.port());
        let result = client.send_message(&Message::Status).await;
        assert!(matches!(result, Err(IpcError::EmptyResponse)));

        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_send_message_returns_protocol_error_on_incomplete_frame() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            loop {
                let n = stream.read(&mut byte).await.unwrap();
                if n == 0 {
                    break;
                }
                request.push(byte[0]);
                if byte[0] == b'\n' {
                    break;
                }
            }

            let partial = br#"{\"type\":\"Ok\",\"message\":\"partial\"}"#;
            stream.write_all(partial).await.unwrap();
        });

        let client = IpcClient::new(addr.ip().to_string(), addr.port());
        let result = client.send_message(&Message::Status).await;
        match result {
            Err(IpcError::ProtocolError(msg)) => {
                assert!(msg.contains("incomplete frame"));
            }
            other => panic!("expected ProtocolError for incomplete frame, got {other:?}"),
        }

        server.await.unwrap();
    }
}

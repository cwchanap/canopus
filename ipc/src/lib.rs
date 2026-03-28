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
        // Also supports legacy daemons that return JSON without newline delimiter
        let mut buffer = Vec::with_capacity(4096);
        loop {
            let chunk = reader
                .fill_buf()
                .await
                .map_err(|e| IpcError::ReceiveFailed(e.to_string()))?;

            if chunk.is_empty() {
                // Connection closed (EOF)
                if buffer.is_empty() {
                    return Err(IpcError::EmptyResponse);
                }
                // Legacy fallback: parse whatever we have as JSON
                debug!(
                    "Connection closed, parsing {} bytes as legacy response",
                    buffer.len()
                );
                let response: Response = serde_json::from_slice(&buffer).map_err(|e| {
                    IpcError::ProtocolError(format!(
                        "Failed to parse response without newline delimiter: {e}"
                    ))
                })?;
                return Ok(response);
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
                // Found newline delimiter - standard protocol path
                break;
            }

            // Legacy compatibility: try to parse accumulated data as JSON.
            // This handles older daemons that return complete JSON without a newline
            // but keep the connection open. Without this check, we'd block forever
            // waiting for more data that will never arrive.
            if let Ok(response) = serde_json::from_slice::<Response>(&buffer) {
                debug!(
                    "Parsed legacy response (no newline delimiter) of {} bytes",
                    buffer.len()
                );
                return Ok(response);
            }
            // JSON incomplete - continue waiting for more data or newline
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
    async fn test_send_message_legacy_fallback_without_newline_connection_closed() {
        // Test backward compatibility: older daemons may return plain JSON without newline
        // and then close the connection (original legacy fallback path)
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut byte = [0_u8; 1];
            loop {
                let n = stream.read(&mut byte).await.unwrap();
                if n == 0 {
                    break;
                }
                if byte[0] == b'\n' {
                    break;
                }
            }

            // Send valid JSON without newline (legacy daemon behavior)
            // Use correct Response format: {"ok":{"message":"success"}}
            let response = br#"{"ok":{"message":"success"}}"#;
            stream.write_all(response).await.unwrap();
            // Connection closes here without newline
        });

        let client = IpcClient::new(addr.ip().to_string(), addr.port());
        let result = client.send_message(&Message::Status).await;
        // With legacy fallback, this should now succeed
        assert!(
            result.is_ok(),
            "expected Ok with legacy fallback, got {result:?}"
        );

        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_send_message_legacy_fallback_without_newline_connection_open() {
        // Critical test: older daemons may return plain JSON without newline
        // and KEEP the connection open for reuse. Without proactive JSON parsing,
        // the client would block indefinitely waiting for more data.
        // This simulates a rolling upgrade scenario (new CLI / old daemon).
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let (tx, rx) = tokio::sync::oneshot::channel();

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut byte = [0_u8; 1];
            loop {
                let n = stream.read(&mut byte).await.unwrap();
                if n == 0 {
                    break;
                }
                if byte[0] == b'\n' {
                    break;
                }
            }

            // Send valid JSON without newline (legacy daemon behavior)
            let response = br#"{"ok":{"message":"rolling-upgrade-success"}}"#;
            stream.write_all(response).await.unwrap();
            // DO NOT close the connection - keep it open

            // Signal that response was sent
            let _ = tx.send(());

            // Keep connection alive for a while to verify client doesn't block
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        });

        let client = IpcClient::new(addr.ip().to_string(), addr.port());

        // This should NOT hang - should return immediately with parsed response
        let result = tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            client.send_message(&Message::Status),
        )
        .await;

        // Verify server sent the response
        rx.await.expect("Server should have sent response");

        match result {
            Ok(Ok(response)) => {
                // Success - client parsed response without blocking
                assert!(
                    matches!(response, Response::Ok { .. }),
                    "expected Ok response, got {response:?}"
                );
            }
            Ok(Err(e)) => {
                panic!("Expected successful response, got error: {e:?}");
            }
            Err(e) => {
                panic!("Client timed out - legacy parse didn't work with open connection: {e:?}");
            }
        }

        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn test_send_message_returns_protocol_error_on_invalid_json_without_newline() {
        // Test that invalid JSON without newline still returns an error
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut byte = [0_u8; 1];
            loop {
                let n = stream.read(&mut byte).await.unwrap();
                if n == 0 {
                    break;
                }
                if byte[0] == b'\n' {
                    break;
                }
            }

            // Send invalid JSON without newline
            let partial = br#"{"type":"Ok","message":"partial"#;
            stream.write_all(partial).await.unwrap();
            // Connection closes here without newline
        });

        let client = IpcClient::new(addr.ip().to_string(), addr.port());
        let result = client.send_message(&Message::Status).await;
        match result {
            Err(IpcError::ProtocolError(msg)) => {
                assert!(
                    msg.contains("Failed to parse response without newline delimiter"),
                    "expected parse error message, got: {msg}"
                );
            }
            other => {
                panic!("expected ProtocolError for invalid JSON without newline, got {other:?}")
            }
        }

        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_send_message_with_newline_terminated_invalid_json_returns_deserialization_error()
    {
        // When the server sends invalid JSON with a trailing newline, the client
        // should return a DeserializationFailed error (not ProtocolError).
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            // Drain the request
            let mut buf = [0u8; 256];
            loop {
                let n = stream.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                if buf[..n].contains(&b'\n') {
                    break;
                }
            }
            // Send invalid JSON with newline delimiter
            stream.write_all(b"not-valid-json\n").await.unwrap();
        });

        let client = IpcClient::new(addr.ip().to_string(), addr.port());
        let result = client.send_message(&Message::Status).await;
        assert!(
            matches!(result, Err(IpcError::DeserializationFailed(_))),
            "expected DeserializationFailed, got {result:?}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_send_message_with_crlf_delimiter_parses_correctly() {
        // The client should handle \r\n delimiters (strip both \r and \n)
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 256];
            loop {
                let n = stream.read(&mut buf).await.unwrap();
                if n == 0 || buf[..n].contains(&b'\n') {
                    break;
                }
            }
            // Send response with \r\n ending
            let response = b"{\"ok\":{\"message\":\"crlf-ok\"}}\r\n";
            stream.write_all(response).await.unwrap();
        });

        let client = IpcClient::new(addr.ip().to_string(), addr.port());
        let result = client.send_message(&Message::Status).await;
        assert!(
            result.is_ok(),
            "expected Ok for CRLF-terminated response, got {result:?}"
        );
        assert!(
            matches!(result.unwrap(), Response::Ok { ref message } if message == "crlf-ok"),
            "response message should be 'crlf-ok'"
        );
        server.await.unwrap();
    }

    #[test]
    fn test_ipc_client_debug_contains_host_and_port() {
        let client = IpcClient::new("192.168.1.1", 7777);
        let debug_str = format!("{client:?}");
        assert!(
            debug_str.contains("192.168.1.1"),
            "debug output should contain host: {debug_str}"
        );
        assert!(
            debug_str.contains("7777"),
            "debug output should contain port: {debug_str}"
        );
    }

    #[tokio::test]
    async fn test_send_message_rejects_oversized_response() {
        // Verify that IpcClient enforces the MAX_FRAME_SIZE limit.
        // When the server sends more than 64KB without a newline, the client
        // must return IpcError::ProtocolError with "exceeds maximum allowed size".
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();

            // Drain the client's request (read until newline)
            let mut byte = [0_u8; 1];
            loop {
                let n = stream.read(&mut byte).await.unwrap();
                if n == 0 || byte[0] == b'\n' {
                    break;
                }
            }

            // Send MAX_FRAME_SIZE + 1 bytes of garbage without a newline.
            // 64 * 1024 + 1 = 65537 bytes — one byte over the limit.
            let oversized = vec![b'x'; 64 * 1024 + 1];
            stream.write_all(&oversized).await.unwrap();
            // The size check fires immediately upon accumulating enough bytes; dropping the stream here is just cleanup.
        });

        let client = IpcClient::new(addr.ip().to_string(), addr.port());
        let result = client.send_message(&Message::Status).await;

        match result {
            Err(IpcError::ProtocolError(msg)) => {
                assert!(
                    msg.contains("exceeds maximum allowed size"),
                    "expected 'exceeds maximum allowed size' in error, got: {msg}"
                );
            }
            other => {
                panic!("expected ProtocolError for oversized response, got {other:?}");
            }
        }

        server.await.unwrap();
    }
}

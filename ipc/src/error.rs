//! IPC error types and utilities

use thiserror::Error;

/// IPC-specific error types
#[derive(Error, Debug)]
pub enum IpcError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    
    #[error("Failed to send message: {0}")]
    SendFailed(String),
    
    #[error("Failed to receive response: {0}")]
    ReceiveFailed(String),
    
    #[error("Serialization failed: {0}")]
    SerializationFailed(String),
    
    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),
    
    #[error("Received empty response")]
    EmptyResponse,
    
    #[error("Protocol error: {0}")]
    ProtocolError(String),
    
    #[error("Timeout error: {0}")]
    Timeout(String),
}

impl IpcError {
    /// Get error code for this error type
    pub fn code(&self) -> &'static str {
        match self {
            IpcError::ConnectionFailed(_) => "IPC001",
            IpcError::SendFailed(_) => "IPC002",
            IpcError::ReceiveFailed(_) => "IPC003",
            IpcError::SerializationFailed(_) => "IPC004",
            IpcError::DeserializationFailed(_) => "IPC005",
            IpcError::EmptyResponse => "IPC006",
            IpcError::ProtocolError(_) => "IPC007",
            IpcError::Timeout(_) => "IPC008",
        }
    }
}

/// IPC-specific result type
pub type Result<T> = std::result::Result<T, IpcError>;

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_error_codes() {
        assert_eq!(IpcError::ConnectionFailed("test".to_string()).code(), "IPC001");
        assert_eq!(IpcError::SendFailed("test".to_string()).code(), "IPC002");
        assert_eq!(IpcError::ReceiveFailed("test".to_string()).code(), "IPC003");
        assert_eq!(IpcError::SerializationFailed("test".to_string()).code(), "IPC004");
        assert_eq!(IpcError::DeserializationFailed("test".to_string()).code(), "IPC005");
        assert_eq!(IpcError::EmptyResponse.code(), "IPC006");
        assert_eq!(IpcError::ProtocolError("test".to_string()).code(), "IPC007");
        assert_eq!(IpcError::Timeout("test".to_string()).code(), "IPC008");
    }
    
    #[test]
    fn test_error_display() {
        let error = IpcError::ConnectionFailed("connection refused".to_string());
        assert_eq!(error.to_string(), "Connection failed: connection refused");
    }
}

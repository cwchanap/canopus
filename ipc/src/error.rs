//! IPC error types and utilities

use thiserror::Error;

/// IPC-specific error types
#[derive(Error, Debug)]
pub enum IpcError {
    /// Network connection establishment failures
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Message transmission failures
    #[error("Failed to send message: {0}")]
    SendFailed(String),

    /// Response reception failures
    #[error("Failed to receive response: {0}")]
    ReceiveFailed(String),

    /// Data serialization failures
    #[error("Serialization failed: {0}")]
    SerializationFailed(String),

    /// Data deserialization failures
    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),

    /// No response data received
    #[error("Received empty response")]
    EmptyResponse,

    /// Protocol-level communication errors
    #[error("Protocol error: {0}")]
    ProtocolError(String),

    /// Operation timeout errors
    #[error("Timeout error: {0}")]
    Timeout(String),
}

impl IpcError {
    /// Get error code for this error type
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ConnectionFailed(_) => "IPC001",
            Self::SendFailed(_) => "IPC002",
            Self::ReceiveFailed(_) => "IPC003",
            Self::SerializationFailed(_) => "IPC004",
            Self::DeserializationFailed(_) => "IPC005",
            Self::EmptyResponse => "IPC006",
            Self::ProtocolError(_) => "IPC007",
            Self::Timeout(_) => "IPC008",
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
        assert_eq!(
            IpcError::ConnectionFailed("test".to_string()).code(),
            "IPC001"
        );
        assert_eq!(IpcError::SendFailed("test".to_string()).code(), "IPC002");
        assert_eq!(IpcError::ReceiveFailed("test".to_string()).code(), "IPC003");
        assert_eq!(
            IpcError::SerializationFailed("test".to_string()).code(),
            "IPC004"
        );
        assert_eq!(
            IpcError::DeserializationFailed("test".to_string()).code(),
            "IPC005"
        );
        assert_eq!(IpcError::EmptyResponse.code(), "IPC006");
        assert_eq!(IpcError::ProtocolError("test".to_string()).code(), "IPC007");
        assert_eq!(IpcError::Timeout("test".to_string()).code(), "IPC008");
    }

    #[test]
    fn test_error_display() {
        let error = IpcError::ConnectionFailed("connection refused".to_string());
        assert_eq!(error.to_string(), "Connection failed: connection refused");
    }

    #[test]
    fn send_failed_display() {
        let e = IpcError::SendFailed("broken pipe".to_string());
        assert_eq!(e.to_string(), "Failed to send message: broken pipe");
    }

    #[test]
    fn receive_failed_display() {
        let e = IpcError::ReceiveFailed("eof".to_string());
        assert_eq!(e.to_string(), "Failed to receive response: eof");
    }

    #[test]
    fn serialization_failed_display() {
        let e = IpcError::SerializationFailed("invalid type".to_string());
        assert_eq!(e.to_string(), "Serialization failed: invalid type");
    }

    #[test]
    fn deserialization_failed_display() {
        let e = IpcError::DeserializationFailed("unexpected token".to_string());
        assert_eq!(e.to_string(), "Deserialization failed: unexpected token");
    }

    #[test]
    fn empty_response_display() {
        let e = IpcError::EmptyResponse;
        assert_eq!(e.to_string(), "Received empty response");
    }

    #[test]
    fn protocol_error_display() {
        let e = IpcError::ProtocolError("missing handshake".to_string());
        assert_eq!(e.to_string(), "Protocol error: missing handshake");
    }

    #[test]
    fn timeout_display() {
        let e = IpcError::Timeout("30s elapsed".to_string());
        assert_eq!(e.to_string(), "Timeout error: 30s elapsed");
    }

    #[test]
    fn all_variants_have_unique_codes() {
        let variants: &[(&str, &str)] = &[
            ("IPC001", IpcError::ConnectionFailed("".into()).code()),
            ("IPC002", IpcError::SendFailed("".into()).code()),
            ("IPC003", IpcError::ReceiveFailed("".into()).code()),
            ("IPC004", IpcError::SerializationFailed("".into()).code()),
            ("IPC005", IpcError::DeserializationFailed("".into()).code()),
            ("IPC006", IpcError::EmptyResponse.code()),
            ("IPC007", IpcError::ProtocolError("".into()).code()),
            ("IPC008", IpcError::Timeout("".into()).code()),
        ];
        for (expected, actual) in variants {
            assert_eq!(expected, actual);
        }
        // Ensure all codes are unique
        let codes: std::collections::HashSet<_> = variants.iter().map(|(_, c)| *c).collect();
        assert_eq!(codes.len(), variants.len(), "all error codes should be unique");
    }

    #[test]
    fn error_source_is_none_for_non_wrapping_variants() {
        use std::error::Error;
        assert!(IpcError::ConnectionFailed("x".into()).source().is_none());
        assert!(IpcError::SendFailed("x".into()).source().is_none());
        assert!(IpcError::EmptyResponse.source().is_none());
        assert!(IpcError::ProtocolError("x".into()).source().is_none());
    }
}

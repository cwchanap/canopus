//! Simple daemon error types

/// Daemon-specific error types
#[derive(Debug)]
pub enum DaemonError {
    /// Server startup and operation errors
    ServerError(String),
    /// Client connection handling errors
    ConnectionError(String),
    /// I/O operation errors
    IoError(std::io::Error),
    /// JSON serialization/deserialization errors
    SerializationError(serde_json::Error),
}

impl std::fmt::Display for DaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DaemonError::ServerError(msg) => write!(f, "Server error: {}", msg),
            DaemonError::ConnectionError(msg) => write!(f, "Connection error: {}", msg),
            DaemonError::IoError(err) => write!(f, "I/O error: {}", err),
            DaemonError::SerializationError(err) => write!(f, "Serialization error: {}", err),
        }
    }
}

impl std::error::Error for DaemonError {}

impl From<std::io::Error> for DaemonError {
    fn from(err: std::io::Error) -> Self {
        DaemonError::IoError(err)
    }
}

impl From<serde_json::Error> for DaemonError {
    fn from(err: serde_json::Error) -> Self {
        DaemonError::SerializationError(err)
    }
}

/// Result type for daemon operations
pub type Result<T> = std::result::Result<T, DaemonError>;

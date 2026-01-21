//! CLI error types

use thiserror::Error;

/// CLI-specific error types
#[derive(Error, Debug)]
pub enum CliError {
    /// Command execution failures
    #[error("Command failed: {0}")]
    CommandFailed(String),

    /// Invalid command line arguments
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    /// Configuration file or parameter errors
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Network connection failures
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Daemon operation errors
    #[error("Daemon error: {0}")]
    DaemonError(String),

    /// IPC communication errors
    #[error("IPC error: {0}")]
    IpcError(#[source] ipc::IpcError),

    /// I/O operation errors
    #[error("I/O error: {0}")]
    IoError(#[source] std::io::Error),
}

impl CliError {
    /// Get error code for this error type
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::CommandFailed(_) => "CLI001",
            Self::InvalidArgument(_) => "CLI002",
            Self::ConfigError(_) => "CLI003",
            Self::ConnectionFailed(_) => "CLI004",
            Self::DaemonError(_) => "CLI005",
            Self::IpcError(_) => "CLI007",
            Self::IoError(_) => "CLI008",
        }
    }
}

/// CLI-specific result type
pub type Result<T> = std::result::Result<T, CliError>;

impl From<ipc::IpcError> for CliError {
    fn from(err: ipc::IpcError) -> Self {
        Self::IpcError(err)
    }
}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes() {
        assert_eq!(CliError::CommandFailed("test".to_string()).code(), "CLI001");
        assert_eq!(
            CliError::InvalidArgument("test".to_string()).code(),
            "CLI002"
        );
        assert_eq!(CliError::ConfigError("test".to_string()).code(), "CLI003");
        assert_eq!(
            CliError::ConnectionFailed("test".to_string()).code(),
            "CLI004"
        );
        assert_eq!(CliError::DaemonError("test".to_string()).code(), "CLI005");
    }

    #[test]
    fn test_error_display() {
        let error = CliError::CommandFailed("invalid command".to_string());
        assert_eq!(error.to_string(), "Command failed: invalid command");
    }
}

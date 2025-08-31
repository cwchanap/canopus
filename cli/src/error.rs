//! CLI error types

use thiserror::Error;

/// CLI-specific error types
#[derive(Error, Debug)]
pub enum CliError {
    #[error("Command failed: {0}")]
    CommandFailed(String),
    
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    
    #[error("Configuration error: {0}")]
    ConfigError(String),
    
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    
    #[error("Daemon error: {0}")]
    DaemonError(String),
    
    #[error("IPC error: {0}")]
    IpcError(#[from] ipc::IpcError),
    
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

impl CliError {
    /// Get error code for this error type
    pub fn code(&self) -> &'static str {
        match self {
            CliError::CommandFailed(_) => "CLI001",
            CliError::InvalidArgument(_) => "CLI002",
            CliError::ConfigError(_) => "CLI003",
            CliError::ConnectionFailed(_) => "CLI004",
            CliError::DaemonError(_) => "CLI005",
            CliError::IpcError(_) => "CLI007",
            CliError::IoError(_) => "CLI008",
        }
    }
}

/// CLI-specific result type
pub type Result<T> = std::result::Result<T, CliError>;

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_error_codes() {
        assert_eq!(CliError::CommandFailed("test".to_string()).code(), "CLI001");
        assert_eq!(CliError::InvalidArgument("test".to_string()).code(), "CLI002");
        assert_eq!(CliError::ConfigError("test".to_string()).code(), "CLI003");
        assert_eq!(CliError::ConnectionFailed("test".to_string()).code(), "CLI004");
        assert_eq!(CliError::DaemonError("test".to_string()).code(), "CLI005");
    }
    
    #[test]
    fn test_error_display() {
        let error = CliError::CommandFailed("invalid command".to_string());
        assert_eq!(error.to_string(), "Command failed: invalid command");
    }
}

//! Core error types and utilities

use thiserror::Error;

/// Core-specific error types
#[derive(Error, Debug)]
pub enum CoreError {
    /// Configuration-related errors
    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    /// Validation errors for input data
    #[error("Validation error: {0}")]
    ValidationError(String),

    /// Initialization and setup errors
    #[error("Initialization error: {0}")]
    InitializationError(String),

    /// Service operation errors
    #[error("Service error: {0}")]
    ServiceError(String),

    /// I/O operation errors
    #[error("I/O error: {0}")]
    IoError(#[source] std::io::Error),

    /// JSON serialization/deserialization errors
    #[error("Serialization error: {0}")]
    SerializationError(#[source] serde_json::Error),

    /// Port allocation errors
    #[error("Port {0} is already in use")]
    PortInUse(u16),

    /// No available port found after trying multiple options
    #[error("No available port found after trying {tried} ports")]
    NoAvailablePort {
        /// Number of ports that were tried
        tried: usize,
    },

    /// Process spawning errors
    #[error("Process spawn error: {0}")]
    ProcessSpawn(String),

    /// Process signaling errors
    #[error("Process signal error: {0}")]
    ProcessSignal(String),

    /// Process waiting/status errors
    #[error("Process wait error: {0}")]
    ProcessWait(String),

    /// Generic or unspecified errors
    #[error("Generic error: {0}")]
    Other(String),
}

impl CoreError {
    /// Get error code for this error type
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ConfigurationError(_) => "CORE001",
            Self::ValidationError(_) => "CORE002",
            Self::InitializationError(_) => "CORE003",
            Self::ServiceError(_) => "CORE004",
            Self::IoError(_) => "CORE005",
            Self::SerializationError(_) => "CORE006",
            Self::PortInUse(_) => "CORE007",
            Self::NoAvailablePort { .. } => "CORE008",
            Self::ProcessSpawn(_) => "CORE009",
            Self::ProcessSignal(_) => "CORE010",
            Self::ProcessWait(_) => "CORE011",
            Self::Other(_) => "CORE999",
        }
    }
}

/// Core-specific result type
pub type Result<T> = std::result::Result<T, CoreError>;

// Convenience implementations
impl From<&str> for CoreError {
    fn from(s: &str) -> Self {
        Self::Other(s.to_string())
    }
}

impl From<String> for CoreError {
    fn from(s: String) -> Self {
        Self::Other(s)
    }
}

impl From<std::io::Error> for CoreError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err)
    }
}

impl From<serde_json::Error> for CoreError {
    fn from(err: serde_json::Error) -> Self {
        Self::SerializationError(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes() {
        assert_eq!(
            CoreError::ConfigurationError("test".to_string()).code(),
            "CORE001"
        );
        assert_eq!(
            CoreError::ValidationError("test".to_string()).code(),
            "CORE002"
        );
        assert_eq!(
            CoreError::InitializationError("test".to_string()).code(),
            "CORE003"
        );
        assert_eq!(
            CoreError::ServiceError("test".to_string()).code(),
            "CORE004"
        );
        assert_eq!(CoreError::Other("test".to_string()).code(), "CORE999");
    }

    #[test]
    fn test_error_display() {
        let error = CoreError::ConfigurationError("invalid port".to_string());
        assert_eq!(error.to_string(), "Configuration error: invalid port");
    }

    #[test]
    fn test_from_implementations() {
        let error: CoreError = "test error".into();
        assert_eq!(error.to_string(), "Generic error: test error");

        let error: CoreError = "test error".to_string().into();
        assert_eq!(error.to_string(), "Generic error: test error");
    }
}

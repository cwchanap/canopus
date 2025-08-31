//! Core error types and utilities

use thiserror::Error;

/// Core-specific error types
#[derive(Error, Debug)]
pub enum CoreError {
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
    
    #[error("Validation error: {0}")]
    ValidationError(String),
    
    #[error("Initialization error: {0}")]
    InitializationError(String),
    
    #[error("Service error: {0}")]
    ServiceError(String),
    
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    
    #[error("Generic error: {0}")]
    Other(String),
}

impl CoreError {
    /// Get error code for this error type
    pub fn code(&self) -> &'static str {
        match self {
            CoreError::ConfigurationError(_) => "CORE001",
            CoreError::ValidationError(_) => "CORE002", 
            CoreError::InitializationError(_) => "CORE003",
            CoreError::ServiceError(_) => "CORE004",
            CoreError::IoError(_) => "CORE005",
            CoreError::SerializationError(_) => "CORE006",
            CoreError::Other(_) => "CORE999",
        }
    }
}

/// Core-specific result type
pub type Result<T> = std::result::Result<T, CoreError>;

// Convenience implementations
impl From<&str> for CoreError {
    fn from(s: &str) -> Self {
        CoreError::Other(s.to_string())
    }
}

impl From<String> for CoreError {
    fn from(s: String) -> Self {
        CoreError::Other(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_error_codes() {
        assert_eq!(CoreError::ConfigurationError("test".to_string()).code(), "CORE001");
        assert_eq!(CoreError::ValidationError("test".to_string()).code(), "CORE002");
        assert_eq!(CoreError::InitializationError("test".to_string()).code(), "CORE003");
        assert_eq!(CoreError::ServiceError("test".to_string()).code(), "CORE004");
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

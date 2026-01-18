//! Utility functions and helper types for core functionality

use serde_json;
use tracing::info;

/// Initialize tracing for the application
pub fn init_tracing() {
    tracing_subscriber::fmt().with_env_filter("info").init();

    info!("Tracing initialized");
}

/// Validate configuration data
///
/// # Errors
///
/// Returns a validation error if the input is empty or contains invalid JSON.
pub fn validate_config_data(data: &str) -> crate::Result<()> {
    if data.is_empty() {
        return Err(crate::CoreError::ValidationError(
            "Configuration data cannot be empty".to_string(),
        ));
    }

    // Try to parse as JSON to validate structure
    match serde_json::from_str::<serde_json::Value>(data) {
        Ok(_) => Ok(()),
        Err(e) => Err(crate::CoreError::ValidationError(format!("Invalid JSON: {e}"))),
    }
}

/// Common result type for utilities
pub type UtilityResult<T> = Result<T, crate::CoreError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_config_data_empty() {
        let result = validate_config_data("");
        assert!(result.is_err());
        if let Err(crate::CoreError::ValidationError(msg)) = result {
            assert!(msg.contains("empty"));
        }
    }

    #[test]
    fn test_validate_config_data_valid_json() {
        let result = validate_config_data(r#"{"key": "value"}"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_config_data_invalid_json() {
        let result = validate_config_data("{invalid json");
        assert!(result.is_err());
        if let Err(crate::CoreError::ValidationError(msg)) = result {
            assert!(msg.contains("Invalid JSON"));
        }
    }
}

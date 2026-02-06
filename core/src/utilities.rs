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
        Err(e) => Err(crate::CoreError::ValidationError(format!(
            "Invalid JSON: {e}"
        ))),
    }
}

/// Common result type for utilities
pub type UtilityResult<T> = Result<T, crate::CoreError>;

/// Simple pseudo-random number generator (linear congruential generator).
///
/// Avoids adding external RNG dependencies. Suitable for non-cryptographic
/// uses like jitter and mock PIDs but NOT for security-sensitive contexts.
pub mod simple_rng {
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEED: AtomicU64 = AtomicU64::new(1);

    /// Generate a pseudo-random u64
    pub fn next_u64() -> u64 {
        let prev = SEED.load(Ordering::Relaxed);
        let next = prev.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        SEED.store(next, Ordering::Relaxed);
        next
    }

    /// Generate a pseudo-random u32
    #[must_use]
    pub fn next_u32() -> u32 {
        #[allow(clippy::cast_possible_truncation)]
        {
            next_u64() as u32
        }
    }

    /// Generate a pseudo-random f64 in the range [0.0, 1.0)
    #[must_use]
    pub fn next_f64() -> f64 {
        #[allow(clippy::cast_precision_loss)]
        {
            (next_u64() as f64) / (u64::MAX as f64)
        }
    }
}

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

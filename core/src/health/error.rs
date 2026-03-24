//! Error types for health checking operations

use std::time::Duration;
use thiserror::Error;

/// Errors that can occur during health check operations
#[derive(Error, Debug)]
pub enum HealthError {
    /// The health check timed out
    #[error("timeout after {0:?}")]
    Timeout(Duration),

    /// TCP connection failed
    #[error("tcp connection failed: {0}")]
    Tcp(#[source] std::io::Error),

    // HTTP-related errors removed
    /// Unsupported probe type requested
    #[error("unsupported probe type: {0}")]
    UnsupportedProbeType(String),
    // HTTP request/URI errors removed
}

impl From<std::io::Error> for HealthError {
    fn from(err: std::io::Error) -> Self {
        Self::Tcp(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timeout_display_includes_duration_seconds() {
        let err = HealthError::Timeout(Duration::from_secs(5));
        let msg = err.to_string();
        assert!(msg.contains("5s"), "expected '5s' in '{msg}'");
    }

    #[test]
    fn timeout_display_includes_duration_millis() {
        let err = HealthError::Timeout(Duration::from_millis(500));
        let msg = err.to_string();
        assert!(msg.contains("500ms"), "expected '500ms' in '{msg}'");
    }

    #[test]
    fn tcp_display_includes_prefix() {
        let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let err = HealthError::Tcp(io_err);
        let msg = err.to_string();
        assert!(
            msg.contains("tcp connection failed"),
            "expected 'tcp connection failed' in '{msg}'"
        );
    }

    #[test]
    fn unsupported_probe_type_display_includes_type_name() {
        let err = HealthError::UnsupportedProbeType("HttpV2".to_string());
        let msg = err.to_string();
        assert!(msg.contains("HttpV2"), "expected type name in '{msg}'");
        assert!(
            msg.contains("unsupported probe type"),
            "expected 'unsupported probe type' in '{msg}'"
        );
    }

    #[test]
    fn from_io_error_creates_tcp_variant() {
        let io_err = std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out");
        let health_err = HealthError::from(io_err);
        assert!(
            matches!(health_err, HealthError::Tcp(_)),
            "expected Tcp variant, got {health_err:?}"
        );
    }
}

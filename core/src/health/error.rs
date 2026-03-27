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
        let err = HealthError::UnsupportedProbeType("ExecProbe".to_string());
        let msg = err.to_string();
        assert!(
            msg.contains("ExecProbe"),
            "expected probe type name in '{msg}'"
        );
        assert!(
            msg.contains("unsupported probe type"),
            "expected 'unsupported probe type' in '{msg}'"
        );
    }

    #[test]
    fn tcp_error_source_is_io_error() {
        use std::error::Error;
        let io_err = std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out");
        let err = HealthError::Tcp(io_err);
        // HealthError::Tcp wraps an io::Error so source() should return Some
        assert!(
            err.source().is_some(),
            "Tcp variant should have a source error"
        );
    }

    #[test]
    fn timeout_display_with_various_durations() {
        let err_secs = HealthError::Timeout(Duration::from_secs(10));
        let msg = err_secs.to_string();
        assert!(
            msg.contains("10s"),
            "expected '10s' in timeout message '{msg}'"
        );

        let err_micros = HealthError::Timeout(Duration::from_micros(250));
        let msg_micros = err_micros.to_string();
        assert!(!msg_micros.is_empty(), "timeout display should not be empty");
    }

    #[test]
    fn health_error_from_io_error_produces_tcp_variant() {
        let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let health_err: HealthError = io_err.into();
        assert!(
            matches!(health_err, HealthError::Tcp(_)),
            "From<io::Error> should produce Tcp variant"
        );
    }

    #[test]
    fn health_error_debug_format_is_non_empty() {
        let err = HealthError::UnsupportedProbeType("TestProbe".to_string());
        let debug = format!("{err:?}");
        assert!(!debug.is_empty(), "Debug format should not be empty");
        assert!(debug.contains("TestProbe"), "Debug should contain type name");
    }

    #[test]
    fn timeout_display_zero_duration() {
        let err = HealthError::Timeout(Duration::from_secs(0));
        let msg = err.to_string();
        // Zero duration should still display without panic
        assert!(!msg.is_empty(), "zero-duration timeout should display non-empty");
    }
}

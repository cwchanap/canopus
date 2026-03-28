//! Health checking and probing functionality
//!
//! This module provides HTTP and TCP health checking primitives for monitoring
//! service readiness and liveness. It integrates with the schema types to provide
//! a clean interface for the supervisor to execute health checks.
//!
//! ## Types
//!
//! - [`Probe`]: The main trait for health check implementations
//! - [`TcpProbe`]: TCP connection-based health checking
//! - [`HealthError`]: Error types for health check failures
//!
//! ## Integration
//!
//! The module integrates with `schema::HealthCheckType` to provide implementations
//! for the health checks configured in service specifications.

pub mod error;
pub mod tcp;
pub mod types;

// Tests are included inline in each module

pub use error::HealthError;
pub use tcp::TcpProbe;
pub use types::Probe;

use schema::{HealthCheck, HealthCheckType};
use std::time::Duration;

/// Create a probe from a schema health check type
///
/// This function translates between the schema types and the concrete probe implementations.
/// It extracts the timeout from the health check configuration and applies it to the probe.
///
/// # Errors
///
/// Returns an error if the health check type is not supported.
pub fn create_probe(
    check_type: &HealthCheckType,
    timeout: Duration,
) -> Result<Box<dyn Probe + Send + Sync>, HealthError> {
    match check_type {
        HealthCheckType::Tcp { port } => {
            let probe = TcpProbe::new("127.0.0.1", *port, timeout);
            Ok(Box::new(probe))
        }
        HealthCheckType::Exec { .. } => Err(HealthError::UnsupportedProbeType(
            "Exec probes not yet implemented".to_string(),
        )),
    }
}

/// Run a health check probe
///
/// This is a convenience function that creates a probe from a health check configuration
/// and immediately executes it. This is the main entry point for the supervisor to
/// execute health checks.
///
/// # Errors
///
/// Returns an error if the probe cannot be created or if the check fails.
pub async fn run_probe(health_check: &HealthCheck) -> Result<(), HealthError> {
    let timeout = health_check.timeout();
    let probe = create_probe(&health_check.check_type, timeout)?;
    probe.check().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use schema::HealthCheckType;
    use std::time::Duration;

    #[test]
    fn create_probe_tcp_returns_ok() {
        let check_type = HealthCheckType::Tcp { port: 8080 };
        let result = create_probe(&check_type, Duration::from_secs(1));
        assert!(result.is_ok(), "expected Ok for Tcp probe");
    }

    #[test]
    fn create_probe_exec_returns_unsupported_error() {
        let check_type = HealthCheckType::Exec {
            command: "curl".to_string(),
            args: vec!["-f".to_string(), "http://localhost/health".to_string()],
        };
        let result = create_probe(&check_type, Duration::from_secs(1));
        assert!(result.is_err(), "expected Err for Exec probe");
        // Extract error without unwrap_err() since Box<dyn Probe> isn't Debug
        let Err(err) = result else {
            panic!("expected error")
        };
        assert!(
            matches!(&err, HealthError::UnsupportedProbeType(_)),
            "expected UnsupportedProbeType, got {err:?}"
        );
        assert!(
            err.to_string().contains("not yet implemented"),
            "error message should mention 'not yet implemented', got: {err}"
        );
    }

    #[test]
    fn health_error_display_timeout() {
        let err = HealthError::Timeout(Duration::from_secs(5));
        assert!(
            err.to_string().contains("5s"),
            "expected timeout duration in message, got: {err}"
        );
    }

    #[test]
    fn health_error_display_unsupported() {
        let err = HealthError::UnsupportedProbeType("MyProbe".to_string());
        assert!(
            err.to_string().contains("MyProbe"),
            "expected probe type name in message, got: {err}"
        );
    }

    #[test]
    fn health_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let health_err = HealthError::from(io_err);
        assert!(
            matches!(health_err, HealthError::Tcp(_)),
            "expected Tcp variant, got {health_err:?}"
        );
    }

    #[tokio::test]
    async fn run_probe_exec_returns_unsupported_error() {
        let check = HealthCheck {
            check_type: HealthCheckType::Exec {
                command: "true".to_string(),
                args: vec![],
            },
            interval_secs: 10,
            timeout_secs: 1,
            failure_threshold: 1,
            success_threshold: 1,
        };
        let result = run_probe(&check).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            HealthError::UnsupportedProbeType(_)
        ));
    }

    #[tokio::test]
    async fn run_probe_tcp_fails_when_nothing_listening() {
        use tokio::net::TcpListener;
        // Bind and immediately release a port so it's not listening
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let check = HealthCheck {
            check_type: HealthCheckType::Tcp { port },
            interval_secs: 1,
            timeout_secs: 1,
            failure_threshold: 1,
            success_threshold: 1,
        };
        let result = run_probe(&check).await;
        assert!(
            result.is_err(),
            "probe should fail when nothing is listening"
        );
    }

    #[tokio::test]
    async fn run_probe_tcp_succeeds_when_something_is_listening() {
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Keep the listener alive while the probe runs
        let _server =
            tokio::spawn(async move { while let Ok((_stream, _)) = listener.accept().await {} });

        let check = HealthCheck {
            check_type: HealthCheckType::Tcp { port },
            interval_secs: 1,
            timeout_secs: 2,
            failure_threshold: 1,
            success_threshold: 1,
        };
        let result = run_probe(&check).await;
        assert!(result.is_ok(), "probe should succeed when port is open");
    }
}

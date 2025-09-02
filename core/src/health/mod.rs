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
//! - [`HttpProbe`]: HTTP request-based health checking
//! - [`Expect`]: Expected response criteria for HTTP probes
//! - [`HealthError`]: Error types for health check failures
//!
//! ## Integration
//!
//! The module integrates with `schema::HealthCheckType` to provide implementations
//! for the health checks configured in service specifications.

pub mod error;
pub mod http;
pub mod tcp;
pub mod types;

// Tests are included inline in each module

pub use error::HealthError;
pub use http::HttpProbe;
pub use tcp::TcpProbe;
pub use types::{Expect, Probe};

use schema::{HealthCheck, HealthCheckType};
use std::time::Duration;

/// Create a probe from a schema health check type
///
/// This function translates between the schema types and the concrete probe implementations.
/// It extracts the timeout from the health check configuration and applies it to the probe.
pub fn create_probe(
    check_type: &HealthCheckType,
    timeout: Duration,
) -> Result<Box<dyn Probe + Send + Sync>, HealthError> {
    match check_type {
        HealthCheckType::Tcp { port } => {
            let probe = TcpProbe::new("127.0.0.1", *port, timeout);
            Ok(Box::new(probe))
        }
        HealthCheckType::Http {
            port,
            path,
            success_codes,
        } => {
            let url = format!("http://127.0.0.1:{}{}", port, path);
            let expect = if success_codes.len() == 1 && success_codes[0] == 200 {
                Expect::Status(200)
            } else if success_codes == &[200, 201, 202, 203, 204] {
                Expect::Any2xx
            } else if success_codes.len() == 1 {
                Expect::Status(success_codes[0])
            } else {
                // For multiple specific codes, we'll use the first one for now
                // In a full implementation, we might want to extend Expect to handle multiple codes
                Expect::Status(success_codes[0])
            };
            let probe = HttpProbe::new(url, expect, timeout);
            Ok(Box::new(probe))
        }
        HealthCheckType::Exec { .. } => {
            Err(HealthError::UnsupportedProbeType("Exec probes not yet implemented".to_string()))
        }
    }
}

/// Run a health check probe
///
/// This is a convenience function that creates a probe from a health check configuration
/// and immediately executes it. This is the main entry point for the supervisor to
/// execute health checks.
pub async fn run_probe(health_check: &HealthCheck) -> Result<(), HealthError> {
    let timeout = health_check.timeout();
    let probe = create_probe(&health_check.check_type, timeout)?;
    probe.check().await
}

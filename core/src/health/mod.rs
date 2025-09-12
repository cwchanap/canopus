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
pub fn create_probe(
    check_type: &HealthCheckType,
    timeout: Duration,
) -> Result<Box<dyn Probe + Send + Sync>, HealthError> {
    match check_type {
        HealthCheckType::Tcp { port } => {
            let probe = TcpProbe::new("127.0.0.1", *port, timeout);
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

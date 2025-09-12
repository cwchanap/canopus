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
    Tcp(#[from] std::io::Error),

    // HTTP-related errors removed

    /// Unsupported probe type requested
    #[error("unsupported probe type: {0}")]
    UnsupportedProbeType(String),

    // HTTP request/URI errors removed
}

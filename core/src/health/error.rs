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

    /// HTTP request failed
    #[error("http error: {0}")]
    Http(#[from] hyper::Error),

    /// HTTP status code did not match expectation
    #[error("unexpected status {0}")]
    UnexpectedStatus(u16),

    /// Response body did not contain expected text
    #[error("response body did not contain expected text")]
    BodyMismatch,

    /// Unsupported probe type requested
    #[error("unsupported probe type: {0}")]
    UnsupportedProbeType(String),

    /// URI parsing error for HTTP probes
    #[error("invalid URI: {0}")]
    InvalidUri(#[from] hyper::http::uri::InvalidUri),

    /// HTTP request building error
    #[error("http request error: {0}")]
    HttpRequest(#[from] hyper::http::Error),
}

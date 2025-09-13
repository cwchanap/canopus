//! Core types and traits for health checking

use super::HealthError;
use async_trait::async_trait;

/// Trait for health check implementations
///
/// This trait is implemented by specific probe types (TCP, HTTP, etc.)
/// to provide a uniform interface for health checking.
#[async_trait]
pub trait Probe {
    /// Execute the health check
    ///
    /// Returns `Ok(())` if the check passes, or an error describing what went wrong.
    /// The implementation should respect the configured timeout.
    async fn check(&self) -> Result<(), HealthError>;
}

#[cfg(test)]
mod tests {
    // No HTTP-specific expectations to test
}

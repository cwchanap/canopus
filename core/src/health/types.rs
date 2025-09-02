//! Core types and traits for health checking

use async_trait::async_trait;
use super::HealthError;

/// Expectation for HTTP response validation
#[derive(Debug, Clone, PartialEq)]
pub enum Expect {
    /// Accept any 2xx status code (200-299)
    Any2xx,
    /// Require a specific status code
    Status(u16),
    /// Require response body to contain specific text
    BodyContains(String),
}

impl Expect {
    /// Check if a status code matches this expectation
    pub fn matches_status(&self, status: u16) -> bool {
        match self {
            Expect::Any2xx => (200..=299).contains(&status),
            Expect::Status(expected) => status == *expected,
            Expect::BodyContains(_) => {
                // For body contains, any 2xx status is acceptable
                // The actual body check happens elsewhere
                (200..=299).contains(&status)
            }
        }
    }

    /// Check if response body matches this expectation
    pub fn matches_body(&self, body: &str) -> bool {
        match self {
            Expect::Any2xx | Expect::Status(_) => true, // Body not relevant
            Expect::BodyContains(expected) => body.contains(expected),
        }
    }
}

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
    use super::*;

    #[test]
    fn test_expect_matches_status() {
        // Test Any2xx
        let any2xx = Expect::Any2xx;
        assert!(any2xx.matches_status(200));
        assert!(any2xx.matches_status(201));
        assert!(any2xx.matches_status(299));
        assert!(!any2xx.matches_status(199));
        assert!(!any2xx.matches_status(300));
        assert!(!any2xx.matches_status(404));

        // Test specific status
        let status200 = Expect::Status(200);
        assert!(status200.matches_status(200));
        assert!(!status200.matches_status(201));
        assert!(!status200.matches_status(404));

        // Test body contains (accepts any 2xx)
        let body_check = Expect::BodyContains("healthy".to_string());
        assert!(body_check.matches_status(200));
        assert!(body_check.matches_status(201));
        assert!(!body_check.matches_status(404));
        assert!(!body_check.matches_status(500));
    }

    #[test]
    fn test_expect_matches_body() {
        let body = "Service is healthy and running";

        // Any2xx and Status don't care about body
        assert!(Expect::Any2xx.matches_body(body));
        assert!(Expect::Status(200).matches_body(body));

        // BodyContains checks for substring
        let contains_healthy = Expect::BodyContains("healthy".to_string());
        assert!(contains_healthy.matches_body(body));

        let contains_error = Expect::BodyContains("error".to_string());
        assert!(!contains_error.matches_body(body));
    }
}

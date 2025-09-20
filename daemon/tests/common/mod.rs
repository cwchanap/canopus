//! Test utilities for integration tests in the daemon crate.

use std::time::Duration;

/// Run the given future with a timeout, failing the test if it elapses.
pub async fn run_with_timeout<F, T>(duration: Duration, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    tokio::time::timeout(duration, fut)
        .await
        .expect("test timed out")
}

/// Run a future with a default timeout of 60 seconds.
pub async fn run_with_default_timeout<F, T>(fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    run_with_timeout(Duration::from_secs(60), fut).await
}

#![allow(unused_crate_dependencies)]
//! Test utilities for CLI crate integration tests.
#![allow(missing_docs)]

use std::time::Duration;

/// Run the given future with a timeout, failing the test if it elapses.
///
/// # Panics
///
/// Panics if the timeout elapses before the future completes.
pub async fn run_with_timeout<F, T>(duration: Duration, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    tokio::time::timeout(duration, fut)
        .await
        .expect("test timed out")
}

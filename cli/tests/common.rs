#![allow(unused_crate_dependencies)]
//! Test utilities for CLI crate integration tests.

use std::time::Duration;

pub async fn run_with_timeout<F, T>(duration: Duration, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    tokio::time::timeout(duration, fut)
        .await
        .expect("test timed out")
}

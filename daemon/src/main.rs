//! Canopus daemon binary
//!
//! The main daemon process that provides system services.

#![allow(unused_crate_dependencies)]

use daemon::Daemon;
use schema::DaemonConfig;
use tracing::{error, info};

#[tokio::main]
async fn main() -> daemon::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    info!("Starting Canopus Daemon");

    // Load configuration (in a real app, this might come from a config file)
    let config = DaemonConfig::default();

    // Create and start the daemon
    let daemon = Daemon::new(config);

    // Handle graceful shutdown
    let daemon_clone = daemon.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for Ctrl+C");
        info!("Received Ctrl+C, shutting down...");
        daemon_clone.stop();
    });

    if let Err(e) = daemon.start().await {
        error!("Daemon failed: {}", e);
        return Err(e);
    }

    info!("Daemon stopped");
    Ok(())
}

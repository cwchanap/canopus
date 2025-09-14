//! Canopus daemon binary
//!
//! The main daemon process that provides system services.

#![allow(unused_crate_dependencies)]

use clap::Parser;
use daemon::{bootstrap, Daemon};
use schema::DaemonConfig;
use tracing::{error, info};

// bootstrap module is provided by the daemon library

/// Daemon CLI options
#[derive(Debug, Parser)]
#[command(name = "canopus-daemon", version, about = "Canopus daemon")]
struct Opts {
    /// Path to services TOML configuration
    #[arg(short, long)]
    config: Option<std::path::PathBuf>,
    /// Host to bind the daemon to
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// Port to bind the daemon to
    #[arg(long, default_value_t = 8080)]
    port: u16,
}

#[tokio::main]
async fn main() -> daemon::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    let opts = Opts::parse();
    info!("Starting Canopus Daemon");

    // Load configuration (in a real app, this might come from a config file)
    let mut config = DaemonConfig::default();
    config.host = opts.host.clone();
    config.port = opts.port;

    // Create and start the daemon (TCP prototype server)
    let daemon = Daemon::new(config);
    // Bootstrap supervisors + IPC + proxy
    let boot = bootstrap::bootstrap(opts.config.clone()).await?;

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
        // try graceful shutdown of bootstrap on error path
        boot.shutdown().await;
        return Err(e);
    }

    info!("Daemon stopped");
    // Gracefully shutdown supervised components
    boot.shutdown().await;
    Ok(())
}

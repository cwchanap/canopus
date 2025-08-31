use core::Config;
use daemon::Daemon;
use tracing::{info, error};
use tracing_subscriber;

#[tokio::main]
async fn main() -> core::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    info!("Starting Canopus Daemon");

    // Load configuration (in a real app, this might come from a config file)
    let config = Config::default();
    
    // Create and start the daemon
    let daemon = Daemon::new(config);
    
    // Handle graceful shutdown
    let daemon_clone = daemon.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
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

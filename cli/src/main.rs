use clap::{Parser, Subcommand};
use core::Config;
use cli::Client;
use tracing::error;
use tracing_subscriber;

#[derive(Parser)]
#[command(name = "canopus")]
#[command(about = "A CLI tool to manage the Canopus daemon")]
#[command(version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Daemon host
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Daemon port
    #[arg(long, default_value_t = 8080)]
    port: u16,
}

#[derive(Subcommand)]
enum Commands {
    /// Get daemon status
    Status,
    /// Start the daemon
    Start,
    /// Stop the daemon
    Stop,
    /// Restart the daemon
    Restart,
    /// Send a custom command to the daemon
    Custom {
        /// The custom command to send
        command: String,
    },
}

#[tokio::main]
async fn main() -> core::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let config = Config {
        daemon_host: cli.host,
        daemon_port: cli.port,
        log_level: "info".to_string(),
    };

    let client = Client::new(config);

    let result = match &cli.command {
        Commands::Status => client.status().await,
        Commands::Start => client.start().await,
        Commands::Stop => client.stop().await,
        Commands::Restart => client.restart().await,
        Commands::Custom { command } => client.custom(command).await,
    };

    if let Err(e) = result {
        error!("Command failed: {}", e);
        std::process::exit(1);
    }

    Ok(())
}

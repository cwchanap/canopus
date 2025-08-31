//! xtask automation tool for Canopus
//!
//! This tool provides various development automation tasks like schema generation.

mod gen_schemas;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask")]
#[command(about = "Development automation tool for Canopus")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate JSON schemas and TypeScript definitions
    GenSchemas,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::GenSchemas => gen_schemas::run(),
    }
}

use std::net::SocketAddr;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "synapse", about = "AI code execution sandbox")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Start the Synapse server
    Serve {
        /// Listen address
        #[arg(long, default_value = "127.0.0.1:8080")]
        listen: SocketAddr,
    },
    /// Runtime related commands (placeholder)
    Runtime,
    /// Check system requirements (placeholder)
    Doctor,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Serve { listen } => {
            synapse_api::server::serve(listen).await?;
        }
        Commands::Runtime => {
            println!("runtime commands are not implemented yet");
        }
        Commands::Doctor => {
            println!("doctor command is not implemented yet");
        }
    }
    Ok(())
}

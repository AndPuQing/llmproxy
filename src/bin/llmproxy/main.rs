use clap::{Parser, Subcommand};
use llmproxy::client::Client;

const BASE_URL: &str = "http://127.0.0.1:11450";

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Register a new model service with the orchestrator
    Register {
        #[arg(long, help = "Name of the model (e.g., Qwen/Qwen2-7B-Instruct)")]
        model_name: String,
        #[arg(long, help = "Address of the model service (e.g., localhost:8001)")]
        addr: String,
    },
    /// Unregister an existing model service by its address
    Unregister {
        #[arg(
            long,
            help = "Address of the model service to unregister (e.g., localhost:8001)"
        )]
        addr: String,
    },
    /// List all registered model services
    List,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();
    let client = Client::new(BASE_URL.to_string());

    let result = match args.command {
        Commands::Register { model_name, addr } => client.register(model_name, addr).await,
        Commands::Unregister { addr } => client.unregister(addr).await,
        Commands::List => client.list().await,
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }

    Ok(())
}

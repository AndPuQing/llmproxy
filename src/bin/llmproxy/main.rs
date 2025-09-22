use clap::{Parser, Subcommand};
use llmproxy::client::Client;
use colored::*;

const BASE_URL: &str = "http://127.0.0.1:11450";

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Clone)]
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

    let command = args.command.clone();
    let result = match args.command {
        Commands::Register { model_name, addr } => client.register(model_name, addr).await,
        Commands::Unregister { addr } => client.unregister(addr).await,
        Commands::List => client.list().await,
    };

    if let Err(e) = result {
        handle_error(&e, &command);
    }

    Ok(())
}

fn handle_error(e: &Box<dyn std::error::Error>, command: &Commands) {
    let error_msg = e.to_string();

    if error_msg.contains("Connection refused")
        || error_msg.contains("connection")
        || error_msg.contains("error sending request") {
        eprintln!("{} {}",
            "✖".red().bold(),
            "Cannot connect to llmproxyd server".red()
        );
        eprintln!("  {} Make sure the server is running on {}",
            "→".bright_blue(),
            BASE_URL.bright_cyan()
        );
        eprintln!("  {} Start it with: {}",
            "→".bright_blue(),
            "llmproxyd".bright_green()
        );
    } else if error_msg.contains("timeout") {
        eprintln!("{} {}",
            "✖".red().bold(),
            "Request timed out".red()
        );
        eprintln!("  {} The server may be overloaded or unresponsive",
            "→".bright_blue()
        );
    } else if error_msg.contains("json") || error_msg.contains("parse") {
        eprintln!("{} {}",
            "✖".red().bold(),
            "Invalid response from server".red()
        );
        eprintln!("  {} Server may be incompatible or corrupted",
            "→".bright_blue()
        );
    } else {
        let operation = match command {
            Commands::Register { .. } => "registration",
            Commands::Unregister { .. } => "unregistration",
            Commands::List => "listing services",
        };

        eprintln!("{} {} failed",
            "✖".red().bold(),
            format!("{}{}", operation.chars().next().unwrap().to_uppercase(), &operation[1..]).red()
        );
        eprintln!("  {} {}",
            "→".bright_blue(),
            error_msg.bright_red()
        );
    }
}

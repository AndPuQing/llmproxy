use clap::{Parser, Subcommand};
use reqwest::Client;
use serde::Deserialize;

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

// To parse responses like {"message": "..."} or {"error": "..."}
#[derive(Deserialize, Debug)]
struct ServerResponse {
    message: Option<String>,
    error: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();
    let client = Client::new();

    match args.command {
        Commands::Register { model_name, addr } => {
            // Check if the server is running before sending requests
            if let Err(_e) = check_server_status(&client).await {
                eprintln!("Make sure the server is running.");
                return Ok(());
            }

            let url = format!("{}/register", BASE_URL);
            let response = client
                .post(&url)
                .json(&serde_json::json!({ "addr": &addr, "model_name": &model_name }))
                .send()
                .await?;

            let status = response.status();
            match response.json::<ServerResponse>().await {
                Ok(parsed_response) => {
                    if status.is_success() {
                        if let Some(msg) = parsed_response.message {
                            println!("Success ({}): {}", status, msg);
                        } else if let Some(err_msg) = parsed_response.error {
                            // Server might return 200 OK but with an error field if API is unusual
                            println!("Server reported error ({}): {}", status, err_msg);
                        } else {
                            println!(
                                "Registered ({}). Server sent an unexpected JSON structure.",
                                status
                            );
                        }
                    } else {
                        if let Some(err_msg) = parsed_response.error {
                            println!("Failed ({}): {}", status, err_msg);
                        } else {
                            println!(
                                "Failed ({}). Server sent an unexpected JSON error structure.",
                                status
                            );
                        }
                    }
                }
                Err(e) => {
                    println!(
                        "Failed to parse server response (Status: {}). Error: {}.",
                        status, e,
                    );
                }
            }
        }
        Commands::Unregister { addr } => {
            // Check if the server is running before sending requests
            if let Err(_e) = check_server_status(&client).await {
                eprintln!("Make sure the server is running.");
                return Ok(());
            }

            let url = format!("{}/unregister", BASE_URL);
            // The backend /unregister endpoint expects RegisterRequest, so it needs model_name.
            // Since the server logic for unregister doesn't use model_name, we send an empty one.
            // The server's unregister handler does not validate model_name for emptiness.
            let response = client
                .post(&url)
                .json(&serde_json::json!({ "model_name": "", "addr": &addr }))
                .send()
                .await?;

            let status = response.status();
            match response.json::<ServerResponse>().await {
                Ok(parsed_response) => {
                    if status.is_success() {
                        if let Some(msg) = parsed_response.message {
                            println!("Success ({}): {}", status, msg);
                        } else if let Some(err_msg) = parsed_response.error {
                            println!("Server reported error ({}): {}", status, err_msg);
                        } else {
                            println!(
                                "Unregistered ({}). Server sent an unexpected JSON structure.",
                                status
                            );
                        }
                    } else {
                        if let Some(err_msg) = parsed_response.error {
                            println!("Failed ({}): {}", status, err_msg);
                        } else {
                            println!(
                                "Failed ({}). Server sent an unexpected JSON error structure.",
                                status
                            );
                        }
                    }
                }
                Err(e) => {
                    println!(
                        "Failed to parse server response (Status: {}). Error: {}.",
                        status, e,
                    );
                }
            }
        }
        Commands::List => {
            // Check if the server is running before sending requests
            if let Err(_e) = check_server_status(&client).await {
                eprintln!("Make sure the server is running.");
                return Ok(());
            }
            let url = format!("{}/list", BASE_URL);
            let response = client.get(&url).send().await?;

            let status = response.status();
            if status.is_success() {
                // The backend returns a Vec<String> for /list
                let server_list = response.json::<Vec<String>>().await?;
                if server_list.is_empty() {
                    println!("No model services registered.");
                } else {
                    println!("Registered model services ({}):", server_list.len());
                    for item in server_list {
                        println!("  - {}", item);
                    }
                }
            } else {
                // Try to parse error if any
                match response.json::<ServerResponse>().await {
                    Ok(parsed_error) => {
                        if let Some(err_msg) = parsed_error.error {
                            println!("Failed to list services ({}): {}", status, err_msg);
                        } else {
                            println!(
                                "Failed to list services ({}). Unexpected JSON error format.",
                                status
                            );
                        }
                    }
                    Err(e) => {
                        println!(
                            "Failed to parse server response (Status: {}). Error: {}.",
                            status, e,
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

async fn check_server_status(client: &Client) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("{}/health", BASE_URL);
    let _response = client.get(&url).send().await?;

    Ok(())
}

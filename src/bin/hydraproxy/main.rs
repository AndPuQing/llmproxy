use clap::{Parser, Subcommand};
use reqwest::Client;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Register { model_name: String, addr: String },
    // Unregister { addr: String },
    List,
}

#[tokio::main]
async fn main() {
    let args = Cli::parse();
    let client = Client::new();

    match args.command {
        Commands::Register { model_name, addr } => {
            client
                .post("http://127.0.0.1:11450/register")
                .json(&serde_json::json!({ "addr": addr, "model_name": model_name }))
                .send()
                .await
                .unwrap();
            println!("Registered {}", addr);
        }
        // Commands::Unregister { addr } => {
        //     client
        //         .post("http://127.0.0.1:11450/unregister")
        //         .json(&serde_json::json!({ "addr": addr }))
        //         .send()
        //         .await
        //         .unwrap();
        //     println!("Unregistered {}", addr);
        // }
        Commands::List => {
            let resp = client
                .get("http://127.0.0.1:11450/list")
                .send()
                .await
                .unwrap()
                .json::<Vec<String>>()
                .await
                .unwrap();
            println!("Servers: {:?}", resp);
        }
    }
}

use crate::models::{ProxyServerInfo, RegisterRequest, ResponseStatus, ServerResponse};
use colored::*;
use reqwest::Client as ReqwestClient;
use reqwest::StatusCode;

pub struct Client {
    http_client: ReqwestClient,
    base_url: String,
}

impl Client {
    pub fn new(base_url: String) -> Self {
        Self {
            http_client: ReqwestClient::new(),
            base_url,
        }
    }

    async fn check_server_status(&self) -> Result<(), reqwest::Error> {
        let url = format!("{}/health", self.base_url);
        self.http_client
            .get(&url)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn register(
        &self,
        model_name: String,
        addr: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.check_server_status().await?;
        let url = format!("{}/register", self.base_url);
        let response = self
            .http_client
            .post(&url)
            .json(&RegisterRequest {
                model_name: model_name.clone(),
                addr: addr.clone(),
            })
            .send()
            .await?;

        handle_response(response, Some(&format!("Registered {} at {}", model_name, addr))).await
    }

    pub async fn unregister(&self, addr: String) -> Result<(), Box<dyn std::error::Error>> {
        self.check_server_status().await?;
        let url = format!("{}/unregister", self.base_url);
        let response = self
            .http_client
            .post(&url)
            .json(&RegisterRequest {
                model_name: "".to_string(), // The server doesn't use this for unregistering
                addr: addr.clone(),
            })
            .send()
            .await?;

        handle_response(response, Some(&format!("Unregistered service at {}", addr))).await
    }

    pub async fn list(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.check_server_status().await?;
        let url = format!("{}/list", self.base_url);
        let response = self.http_client.get(&url).send().await?;

        let status = response.status();
        if status.is_success() {
            let server_list: Vec<ProxyServerInfo> = response.json().await?;
            if server_list.is_empty() {
                println!("{} {}",
                    "ℹ".bright_blue().bold(),
                    "No model services are currently registered".bright_black()
                );
                println!("  {} Use {} to register a new service",
                    "→".bright_blue(),
                    "llmproxy register --model-name <MODEL> --addr <ADDRESS>".bright_green()
                );
            } else {
                println!("{} {} registered service{}",
                    "✔".green().bold(),
                    server_list.len().to_string().bright_cyan(),
                    if server_list.len() == 1 { "" } else { "s" }
                );
                println!();

                let mut table = comfy_table::Table::new();
                table
                    .set_header(vec!["#", "Model", "Address"])
                    .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
                    .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS);

                for (index, server) in server_list.iter().enumerate() {
                    table.add_row(vec![
                        (index + 1).to_string(),
                        server.model_name.clone(),
                        server.addr.clone(),
                    ]);
                }
                println!("{table}");
            }
        } else {
            handle_error_response(status, response).await?;
        }
        Ok(())
    }
}

async fn handle_response(
    response: reqwest::Response,
    context: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let status = response.status();
    let parsed_response: ServerResponse = response.json().await?;

    if status.is_success() {
        match parsed_response.status {
            ResponseStatus::Success => {
                if let Some(ctx) = context {
                    println!("✔ {}", ctx.green().bold());
                    if !parsed_response.message.is_empty() && parsed_response.message != "OK" {
                        println!("  {} {}", "→".bright_blue(), parsed_response.message.bright_black());
                    }
                } else {
                    println!("✔ {}", parsed_response.message.green());
                }
            }
            ResponseStatus::Warning => {
                println!("⚠ {}", parsed_response.message.yellow().bold());
                if let Some(ctx) = context {
                    println!("  {} {}", "→".bright_blue(), ctx.bright_black());
                }
            }
            ResponseStatus::Error => {
                println!("✖ {} ({})", parsed_response.message.red().bold(), status)
            }
        }
    } else {
        println!(
            "✖ {} ({})",
            parsed_response.message.red().bold(),
            status
        );
    }
    Ok(())
}

async fn handle_error_response(
    status: StatusCode,
    response: reqwest::Response,
) -> Result<(), Box<dyn std::error::Error>> {
    let error_text = response.text().await?;
    println!(
        "✖ {} ({})",
        format!("Server error: {}", error_text).red().bold(),
        status
    );

    if status == StatusCode::NOT_FOUND {
        println!("  {} The requested endpoint may not exist", "→".bright_blue());
    } else if status == StatusCode::INTERNAL_SERVER_ERROR {
        println!("  {} The server encountered an internal error", "→".bright_blue());
    } else if status.is_client_error() {
        println!("  {} Check your request parameters", "→".bright_blue());
    }

    Ok(())
}

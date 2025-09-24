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

        handle_response(
            response,
            Some(&format!("Registered {} at {}", model_name, addr)),
        )
        .await
    }

    pub async fn unregister(&self, target: String) -> Result<(), Box<dyn std::error::Error>> {
        self.check_server_status().await?;

        // Check if the input is a number (index) or an address
        let actual_addr = if target.parse::<usize>().is_ok() {
            self.resolve_index_to_address(&target).await?
        } else {
            target.clone()
        };

        let url = format!("{}/unregister", self.base_url);
        let response = self
            .http_client
            .post(&url)
            .json(&RegisterRequest {
                model_name: "".to_string(), // The server doesn't use this for unregistering
                addr: actual_addr.clone(),
            })
            .send()
            .await?;

        let context = if target.parse::<usize>().is_ok() {
            format!("Unregistered service #{} ({})", target, actual_addr)
        } else {
            format!("Unregistered service at {}", actual_addr)
        };

        handle_response(response, Some(&context)).await
    }

    async fn resolve_index_to_address(
        &self,
        index_str: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let index: usize = index_str
            .parse()
            .map_err(|_| format!("Invalid index '{}'", index_str))?;

        if index == 0 {
            return Err("Service indices start from 1, not 0".to_string().into());
        }

        // Get the current list of services
        let url = format!("{}/list", self.base_url);
        let response = self.http_client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err("Failed to retrieve service list to resolve index".into());
        }

        let server_list: Vec<ProxyServerInfo> = response.json().await?;

        if server_list.is_empty() {
            return Err("No services are registered".into());
        }

        if index > server_list.len() {
            return Err(format!(
                "Index {} not found. Only {} service{} registered.",
                index,
                server_list.len(),
                if server_list.len() == 1 {
                    " is"
                } else {
                    "s are"
                }
            )
            .into());
        }

        Ok(server_list[index - 1].addr.clone())
    }

    pub async fn list(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.check_server_status().await?;
        let url = format!("{}/list", self.base_url);
        let response = self.http_client.get(&url).send().await?;

        let status = response.status();
        if status.is_success() {
            let server_list: Vec<ProxyServerInfo> = response.json().await?;
            if server_list.is_empty() {
                println!(
                    "{} {}",
                    "ℹ".bright_blue().bold(),
                    "No model services are currently registered".bright_black()
                );
                println!(
                    "  {} Use {} to register a new service",
                    "→".bright_blue(),
                    "llmproxy register --model-name <MODEL> --addr <ADDRESS>".bright_green()
                );
            } else {
                // Calculate column widths
                let label_width = 5; // "Label"
                let mut model_width = 5; // "Model"
                let mut addr_width = 7; // "Address"

                for server in &server_list {
                    model_width = model_width.max(server.model_name.len());
                    addr_width = addr_width.max(server.addr.len());
                }

                // Print header
                println!(
                    "{:<width_label$}  {:<width_model$}  {:<width_addr$}",
                    "Label",
                    "Model",
                    "Address",
                    width_label = label_width,
                    width_model = model_width,
                    width_addr = addr_width
                );

                // Print rows
                for (index, server) in server_list.iter().enumerate() {
                    let label = format!("#{}", index + 1);
                    println!(
                        "{:<width_label$}  {:<width_model$}  {:<width_addr$}",
                        label.bright_cyan(),
                        server.model_name,
                        server.addr,
                        width_label = label_width,
                        width_model = model_width,
                        width_addr = addr_width
                    );
                }

                println!();
                println!(
                    "{} You can unregister services by index or address:",
                    "💡".bright_yellow()
                );
                println!(
                    "  {} {}",
                    "→".bright_blue(),
                    "llmproxy unregister 1".bright_green()
                );
                println!(
                    "  {} {}",
                    "→".bright_blue(),
                    "llmproxy unregister localhost:8001".bright_green()
                );
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
                        println!(
                            "  {} {}",
                            "→".bright_blue(),
                            parsed_response.message.bright_black()
                        );
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
        println!("✖ {} ({})", parsed_response.message.red().bold(), status);
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
        println!(
            "  {} The requested endpoint may not exist",
            "→".bright_blue()
        );
    } else if status == StatusCode::INTERNAL_SERVER_ERROR {
        println!(
            "  {} The server encountered an internal error",
            "→".bright_blue()
        );
    } else if status.is_client_error() {
        println!("  {} Check your request parameters", "→".bright_blue());
    }

    Ok(())
}

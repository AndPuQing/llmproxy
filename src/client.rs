use crate::models::{RegisterRequest, ServerResponse};
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
            .json(&RegisterRequest { model_name, addr })
            .send()
            .await?;

        handle_response(response).await
    }

    pub async fn unregister(&self, addr: String) -> Result<(), Box<dyn std::error::Error>> {
        self.check_server_status().await?;
        let url = format!("{}/unregister", self.base_url);
        let response = self
            .http_client
            .post(&url)
            .json(&RegisterRequest {
                model_name: "".to_string(), // The server doesn't use this for unregistering
                addr,
            })
            .send()
            .await?;

        handle_response(response).await
    }

    pub async fn list(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.check_server_status().await?;
        let url = format!("{}/list", self.base_url);
        let response = self.http_client.get(&url).send().await?;

        let status = response.status();
        if status.is_success() {
            let server_list: Vec<String> = response.json().await?;
            if server_list.is_empty() {
                println!("No model services registered.");
            } else {
                println!("Registered model services ({}):", server_list.len());
                for item in server_list {
                    println!("  - {item}");
                }
            }
        } else {
            handle_error_response(status, response).await?;
        }
        Ok(())
    }
}

async fn handle_response(response: reqwest::Response) -> Result<(), Box<dyn std::error::Error>> {
    let status = response.status();
    if status.is_success() {
        let parsed_response: ServerResponse = response.json().await?;
        if let Some(msg) = parsed_response.message {
            println!("Success ({status}): {msg}");
        } else {
            println!("Operation successful ({status}), but no message received.");
        }
    } else {
        handle_error_response(status, response).await?;
    }
    Ok(())
}

async fn handle_error_response(
    status: StatusCode,
    response: reqwest::Response,
) -> Result<(), Box<dyn std::error::Error>> {
    match response.json::<ServerResponse>().await {
        Ok(parsed_error) => {
            if let Some(err_msg) = parsed_error.error {
                println!("Failed ({status}): {err_msg}");
            } else {
                println!("Failed ({status}). Unexpected JSON error format.");
            }
        }
        Err(_) => {
            println!("Failed ({status}). Could not parse error response.");
        }
    }
    Ok(())
}

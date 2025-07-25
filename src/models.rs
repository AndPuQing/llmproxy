use serde::{Deserialize, Serialize};

/// Represents the payload for registering or unregistering a model server.
/// Used by both the client and the server.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegisterRequest {
    pub model_name: String,
    pub addr: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ResponseStatus {
    Success,
    Warning,
    Error,
}

/// Represents the generic JSON response from the server.
/// Used by the client to parse both success and error messages.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerResponse {
    pub status: ResponseStatus,
    pub message: String,
}

/// Used by the server to extract the model name from the request body.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModelExtractPayload {
    pub model: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProxyServerInfo {
    pub model_name: String,
    pub addr: String,
}

use crate::models::{
    ModelExtractPayload, ProxyServerInfo, RegisterRequest, ResponseStatus, ServerResponse,
    TestRequest,
};
use axum::{
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use hyper::Uri;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use rand::Rng;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tracing;

#[derive(Clone, Debug)]
struct ProxyServer {
    model_name: String,
    addr: String,
}

#[derive(Clone)]
struct AppState {
    servers: Arc<Mutex<Vec<ProxyServer>>>,
    http_client: Client<hyper_util::client::legacy::connect::HttpConnector, axum::body::Body>,
}

pub async fn run(addr: SocketAddr) {
    let http_client = Client::builder(TokioExecutor::new())
        .pool_idle_timeout(Duration::from_secs(30))
        .http2_only(false)
        .build_http();

    let state = AppState {
        servers: Arc::new(Mutex::new(vec![])),
        http_client,
    };

    let app = app(state);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tracing::info!("Listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

fn app(state: AppState) -> Router {
    let api_routes = Router::new()
        .route("/register", post(register_server))
        .route("/unregister", post(unregister_server))
        .route("/health", get(|| async { "OK" }))
        .route("/list", get(list_servers))
        .route("/test", post(test_server));

    let proxy_router = Router::new().fallback(proxy_request_handler);

    Router::new()
        .merge(api_routes)
        .merge(proxy_router)
        .with_state(state)
}

async fn proxy_request_handler(State(state): State<AppState>, original_req: Request) -> Response {
    tracing::trace!(?original_req, "Received proxy request");

    let servers_guard = state.servers.lock().await;
    if servers_guard.is_empty() {
        tracing::warn!("No vLLM servers registered.");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ServerResponse {
                status: ResponseStatus::Error,
                message: "No vLLM servers registered".to_string(),
            }),
        )
            .into_response();
    }

    let (parts, body) = original_req.into_parts();

    let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("Failed to read request body: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                Json(ServerResponse {
                    status: ResponseStatus::Error,
                    message: "Failed to read request body".to_string(),
                }),
            )
                .into_response();
        }
    };

    let model_payload: ModelExtractPayload = match serde_json::from_slice(&body_bytes) {
        Ok(payload) => payload,
        Err(e) => {
            tracing::warn!("Failed to parse JSON body for model extraction: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                Json(ServerResponse {
                    status: ResponseStatus::Error,
                    message: format!("Invalid JSON body: {}", e),
                }),
            )
                .into_response();
        }
    };

    let model_name = match model_payload.model {
        Some(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ => {
            tracing::warn!("Model name missing or empty in request body.");
            return (
                StatusCode::BAD_REQUEST,
                Json(ServerResponse {
                    status: ResponseStatus::Error,
                    message: "Model name is required in the request body".to_string(),
                }),
            )
                .into_response();
        }
    };
    tracing::debug!("Extracted model name: {model_name}");

    let candidate_servers: Vec<&ProxyServer> = servers_guard
        .iter()
        .filter(|server| server.model_name == model_name)
        .collect();

    if candidate_servers.is_empty() {
        tracing::warn!("No server registered for model: {model_name}");
        return (
            StatusCode::BAD_REQUEST, // Or NOT_FOUND
            Json(ServerResponse {
                status: ResponseStatus::Error,
                message: format!("No server registered for model: {model_name}"),
            }),
        )
            .into_response();
    }

    // Randomly select a server
    let selected_server = {
        let mut rng = rand::rng();
        candidate_servers[rng.random_range(0..candidate_servers.len())]
    };
    let target_addr = selected_server.addr.clone();
    // Drop the lock as soon as we don't need it
    drop(servers_guard);

    tracing::debug!("Selected server: {} for model {}", target_addr, model_name);

    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|x| x.as_str())
        .unwrap_or("/");

    let scheme = "http://";
    let host = target_addr
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let target_uri_str = format!("{scheme}{host}{path_and_query}");

    let target_uri: Uri = match target_uri_str.parse() {
        Ok(uri) => uri,
        Err(e) => {
            tracing::error!("Failed to parse target URI '{target_uri_str}': {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ServerResponse {
                    status: ResponseStatus::Error,
                    message: "Failed to construct target URI".to_string(),
                }),
            )
                .into_response();
        }
    };

    let req_body = axum::body::Body::from(body_bytes);

    let mut builder = Request::builder()
        .method(parts.method.clone())
        .uri(target_uri);

    if let Some(headers_mut) = builder.headers_mut() {
        *headers_mut = parts.headers.clone();
    } else {
        tracing::error!("Failed to get mutable headers from builder");
        return (StatusCode::INTERNAL_SERVER_ERROR, "Error building request").into_response();
    }

    let new_req = match builder.body(req_body) {
        Ok(req) => req,
        Err(e) => {
            tracing::error!("Failed to build proxy request: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ServerResponse {
                    status: ResponseStatus::Error,
                    message: "Failed to build proxy request".to_string(),
                }),
            )
                .into_response();
        }
    };

    tracing::debug!(?new_req, "Forwarding request");

    match state.http_client.request(new_req).await {
        Ok(response) => {
            tracing::debug!(status = ?response.status(), "Received response from target");
            response.into_response()
        }
        Err(err) => {
            tracing::error!("Error forwarding request to {}: {}", target_addr, err);
            (
                StatusCode::BAD_GATEWAY,
                Json(ServerResponse {
                    status: ResponseStatus::Error,
                    message: format!("Error forwarding request: {}", err),
                }),
            )
                .into_response()
        }
    }
}

async fn register_server(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> impl IntoResponse {
    let mut servers = state.servers.lock().await;

    if payload.addr.trim().is_empty() || !payload.addr.contains(':') {
        tracing::warn!(
            "Invalid address provided for registration: {}",
            payload.addr
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(ServerResponse {
                status: ResponseStatus::Error,
                message: "Invalid address format. Expected host:port".to_string(),
            }),
        );
    }
    if payload.model_name.trim().is_empty() {
        tracing::warn!("Empty model_name provided for registration");
        return (
            StatusCode::BAD_REQUEST,
            Json(ServerResponse {
                status: ResponseStatus::Error,
                message: "model_name cannot be empty".to_string(),
            }),
        );
    }

    let server_addr = payload.addr.trim().to_string();
    let server_model_name = payload.model_name.trim().to_string();

    if servers
        .iter()
        .any(|s| s.model_name == server_model_name && s.addr == server_addr)
    {
        tracing::info!(
            "Server already registered: model_name={}, addr={}",
            server_model_name,
            server_addr
        );
        return (
            StatusCode::OK,
            Json(ServerResponse {
                status: ResponseStatus::Warning,
                message: "Server already registered".to_string(),
            }),
        );
    }

    tracing::info!(
        "Registering server: model_name={}, addr={}",
        server_model_name,
        server_addr
    );
    servers.push(ProxyServer {
        model_name: server_model_name,
        addr: server_addr,
    });

    (
        StatusCode::CREATED,
        Json(ServerResponse {
            status: ResponseStatus::Success,
            message: "Server registered successfully".to_string(),
        }),
    )
}

async fn unregister_server(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> impl IntoResponse {
    let mut servers = state.servers.lock().await;

    if payload.addr.trim().is_empty() || !payload.addr.contains(':') {
        tracing::warn!(
            "Invalid address provided for unregistration: {}",
            payload.addr
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(ServerResponse {
                status: ResponseStatus::Error,
                message: "Invalid address format. Expected host:port".to_string(),
            }),
        );
    }

    let server_addr = payload.addr.trim().to_string();

    if let Some(pos) = servers.iter().position(|s| s.addr == server_addr) {
        servers.remove(pos);
        tracing::info!("Unregistered server: addr={}", server_addr);
        (
            StatusCode::OK,
            Json(ServerResponse {
                status: ResponseStatus::Success,
                message: "Server unregistered successfully".to_string(),
            }),
        )
    } else {
        tracing::warn!("Server not found for unregistration: addr={}", server_addr);
        (
            StatusCode::NOT_FOUND,
            Json(ServerResponse {
                status: ResponseStatus::Error,
                message: "Server not found".to_string(),
            }),
        )
    }
}

async fn list_servers(State(state): State<AppState>) -> impl IntoResponse {
    let servers = state.servers.lock().await;

    let server_list_display: Vec<ProxyServerInfo> = servers
        .iter()
        .map(|server| ProxyServerInfo {
            model_name: server.model_name.clone(),
            addr: server.addr.clone(),
        })
        .collect();
    Json(server_list_display)
}

async fn test_server(
    State(state): State<AppState>,
    Json(payload): Json<TestRequest>,
) -> impl IntoResponse {
    let servers = state.servers.lock().await;

    let server_addr = payload.addr.trim().to_string();

    if let Some(_server) = servers.iter().find(|s| s.addr == server_addr) {
        let uri = format!("http://{}/health", server_addr)
            .parse::<Uri>()
            .expect("Failed to parse URI");

        match state.http_client.get(uri).await {
            Ok(response) => {
                if response.status().is_success() {
                    (
                        StatusCode::OK,
                        Json(ServerResponse {
                            status: ResponseStatus::Success,
                            message: format!("Service at {} is reachable", server_addr),
                        }),
                    )
                } else {
                    (
                        StatusCode::OK,
                        Json(ServerResponse {
                            status: ResponseStatus::Error,
                            message: format!(
                                "Service at {} returned status {}",
                                server_addr,
                                response.status()
                            ),
                        }),
                    )
                }
            }
            Err(e) => (
                StatusCode::OK,
                Json(ServerResponse {
                    status: ResponseStatus::Error,
                    message: format!("Failed to connect to service at {}: {}", server_addr, e),
                }),
            ),
        }
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(ServerResponse {
                status: ResponseStatus::Error,
                message: "Service not found".to_string(),
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{self, Request, StatusCode},
    };
    use tower::ServiceExt;

    fn test_app_state() -> AppState {
        let http_client = Client::builder(TokioExecutor::new()).build_http();
        AppState {
            servers: Arc::new(Mutex::new(vec![])),
            http_client,
        }
    }

    #[tokio::test]
    async fn test_register_server_ok() {
        let state = test_app_state();
        let app = app(state.clone());

        let payload = RegisterRequest {
            model_name: "test_model".to_string(),
            addr: "localhost:8001".to_string(),
        };

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/register")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let servers = state.servers.lock().await;
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].model_name, "test_model");
        assert_eq!(servers[0].addr, "localhost:8001");
    }

    #[tokio::test]
    async fn test_register_server_already_exists() {
        let state = test_app_state();
        let app = app(state.clone());

        let payload = RegisterRequest {
            model_name: "test_model".to_string(),
            addr: "localhost:8001".to_string(),
        };

        // First registration
        app.clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/register")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Second registration
        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/register")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let server_response: ServerResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(server_response.status, ResponseStatus::Warning);
        assert_eq!(server_response.message, "Server already registered");
    }
}

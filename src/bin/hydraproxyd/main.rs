use axum::{
    extract::Request,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};

use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use serde::Deserialize;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
struct ProxyServer {
    model_name: String,
    addr: String,
}

#[derive(Clone)]
struct AppState {
    servers: Arc<Mutex<Vec<ProxyServer>>>,
}

#[derive(Deserialize)]
struct RegisterRequest {
    model_name: String,
    addr: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!("{}=trace,tower_http=debug", env!("CARGO_CRATE_NAME")).into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let state = AppState {
        servers: Arc::new(Mutex::new(vec![])), // Initialize with an empty vector
    };

    let register_route = Router::new()
        .route("/register", post(register))
        .route("/list", get(list))
        .layer(Extension(state.clone()));

    let vllm_proxy_route = Router::new()
        .fallback(proxy) // This will catch any route not explicitly defined
        .layer(Extension(state));

    let app = Router::new().merge(register_route).merge(vllm_proxy_route);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:11450")
        .await
        .unwrap();
    tracing::info!("Listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

async fn proxy(Extension(state): Extension<AppState>, req: Request) -> Response {
    tracing::trace!(?req);

    // Get registered servers
    let servers = state.servers.lock().await;

    if servers.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "No vllm servers registered",
        )
            .into_response();
    }

    // Extract parts from the original request
    // curl http://localhost:8000/v1/completions \
    // -H "Content-Type: application/json" \
    // -d '{
    //     "model": "Qwen/Qwen2.5-1.5B-Instruct",
    //     "prompt": "San Francisco is a",
    //     "max_tokens": 7,
    //     "temperature": 0
    // }'
    let (parts, body) = req.into_parts();

    // Extract the model name from the request body
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    let body_str = String::from_utf8_lossy(&bytes);
    let json: serde_json::Value = serde_json::from_str(&body_str).unwrap();
    let model_name = json["model"].as_str().unwrap_or_default();
    let body = axum::body::Body::from(bytes);

    let model_name = model_name.trim();
    if model_name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "Model name is required in the request body",
        )
            .into_response();
    }
    tracing::debug!("Extracted model name: {}", model_name);

    let mut host_servers = servers
        .iter()
        .filter(|server| server.model_name == model_name);

    if host_servers.clone().count() == 0 {
        return (
            StatusCode::BAD_REQUEST,
            format!("No server registered for model: {}", model_name),
        )
            .into_response();
    }

    // Randomly select a server from the list of registered servers
    let host_server = host_servers
        .nth(rand::random_range(0..host_servers.clone().count()))
        .unwrap();

    let host_addr = host_server.addr.clone();
    tracing::debug!("Selected server: {}", host_addr);

    // Create a new client request to the selected server
    let client = Client::builder(TokioExecutor::new())
        .pool_idle_timeout(Duration::from_secs(30))
        .http2_only(false)
        .build_http();

    // Get the path and query from the original request
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|x| x.as_str())
        .unwrap_or("/");

    // Build new URI with selected server address
    let uri = format!("http://{}{}", host_addr, path_and_query);
    let uri: hyper::Uri = uri.parse().unwrap();

    // Create a new request with the same method, headers, and body
    let mut new_req = Request::builder().uri(uri).method(parts.method);

    // Copy the headers
    let headers = new_req.headers_mut().unwrap();
    for (name, value) in parts.headers {
        if let Some(name) = name {
            headers.insert(name, value);
        }
    }

    let new_req = new_req.body(body).unwrap();

    tracing::debug!("Forwarding request to: {}", new_req.uri());
    tracing::debug!("Request headers: {:?}", new_req.headers());
    tracing::debug!("Request body: {:?}", new_req.body());
    // Send the request to the vllm server
    match client.request(new_req).await {
        Ok(response) => response.into_response(),
        Err(err) => {
            tracing::error!("Error forwarding request to {}: {}", host_addr, err);
            (
                StatusCode::BAD_GATEWAY,
                format!("Error forwarding request: {}", err),
            )
                .into_response()
        }
    }
}

async fn register(
    Extension(state): Extension<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> impl IntoResponse {
    let mut servers = state.servers.lock().await;
    tracing::info!(
        "Registered server: model_name={}, addr={}",
        payload.model_name,
        payload.addr
    );
    servers.push(ProxyServer {
        model_name: payload.model_name,
        addr: payload.addr,
    });

    (StatusCode::OK, "Server registered successfully")
}

async fn list(Extension(state): Extension<AppState>) -> impl IntoResponse {
    let servers = state.servers.lock().await;
    let server_list: Vec<String> = servers
        .iter()
        .map(|server| format!("{}: {}", server.model_name, server.addr))
        .collect();
    Json(server_list)
}

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
use serde::Deserialize;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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

#[derive(Deserialize, serde::Serialize)]
struct RegisterRequest {
    model_name: String,
    addr: String,
}

#[derive(Deserialize)]
struct ModelExtractPayload {
    model: Option<String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                // Fallback to a default filter if RUST_LOG is not set
                format!(
                    "{}=info,tower_http=debug,vllm_proxy=trace", // Added crate name for specific trace
                    env!("CARGO_PKG_NAME").replace('-', "_")     // Use CARGO_PKG_NAME
                )
                .into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let http_client = Client::builder(TokioExecutor::new())
        .pool_idle_timeout(Duration::from_secs(30))
        .http2_only(false) // Important for many local vLLM setups that might use HTTP/1.1
        .build_http();

    let state = AppState {
        servers: Arc::new(Mutex::new(vec![])), // Initialize with an empty vector
        http_client,
    };

    let api_routes = Router::new()
        .route("/register", post(register_server))
        .route("/unregister", post(unregister_server))
        .route("/health", get(|| async { "OK" }))
        .route("/list", get(list_servers));

    let proxy_router = Router::new().fallback(proxy_request_handler);

    let app = Router::new()
        .merge(api_routes)
        .merge(proxy_router)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:11450")
        .await
        .unwrap();
    tracing::info!("Listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

async fn proxy_request_handler(State(state): State<AppState>, original_req: Request) -> Response {
    tracing::trace!(?original_req, "Received proxy request");

    let servers_guard = state.servers.lock().await;
    if servers_guard.is_empty() {
        tracing::warn!("No vLLM servers registered.");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "No vLLM servers registered"})),
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
    let (parts, body) = original_req.into_parts();

    // Extract the model name from the request body
    // We need to consume the body to read it, then recreate it.
    let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("Failed to read request body: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Failed to read request body"})),
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
                Json(serde_json::json!({"error": "Invalid JSON body", "details": e.to_string()})),
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
                Json(serde_json::json!({"error": "Model name is required in the request body"})),
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
            Json(serde_json::json!({"error": format!("No server registered for model: {model_name}")})),
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
        .unwrap_or("/"); // Should not happen if original_req.uri was valid

    // Ensure target_addr does not start with http:// or https://
    // and prepend http://
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
                Json(serde_json::json!({"error": "Failed to construct target URI"})),
            )
                .into_response();
        }
    };

    // Reconstruct the request for the target server
    // Important: Create a new body from the collected bytes
    let req_body = axum::body::Body::from(body_bytes);

    // Create a new request builder
    // parts.uri = target_uri; // This modifies the original parts, careful if parts is used later
    // let mut new_req = Request::from_parts(parts, req_body).unwrap(); // This might panic if URI is authority form

    // Safer way:
    let mut builder = Request::builder()
        .method(parts.method.clone()) // Clone method
        .uri(target_uri);

    // Copy headers, filtering out host if necessary or letting hyper handle it
    if let Some(headers_mut) = builder.headers_mut() {
        *headers_mut = parts.headers.clone(); // Clone headers
    } else {
        tracing::error!("Failed to get mutable headers from builder");
        return (StatusCode::INTERNAL_SERVER_ERROR, "Error building request").into_response();
    }
    // hyper's client will typically set the Host header correctly based on the URI.
    // If you need specific Host header manipulation, do it here.
    // For example, to remove an existing Host header from the original request:
    // builder.headers_mut().unwrap().remove(hyper::header::HOST);

    let new_req = match builder.body(req_body) {
        Ok(req) => req,
        Err(e) => {
            tracing::error!("Failed to build proxy request: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to build proxy request"})),
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
                Json(serde_json::json!({"error": format!("Error forwarding request: {}", err)})),
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

    // Basic validation for addr (e.g., not empty, contains ':')
    if payload.addr.trim().is_empty() || !payload.addr.contains(':') {
        tracing::warn!(
            "Invalid address provided for registration: {}",
            payload.addr
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid address format. Expected host:port"})),
        );
    }
    if payload.model_name.trim().is_empty() {
        tracing::warn!("Empty model_name provided for registration");
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "model_name cannot be empty"})),
        );
    }

    let server_addr = payload.addr.trim().to_string();
    let server_model_name = payload.model_name.trim().to_string();

    // Optional: Check for duplicates
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
            Json(serde_json::json!({"message": "Server already registered"})),
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
        Json(serde_json::json!({"message": "Server registered successfully"})),
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
            Json(serde_json::json!({"error": "Invalid address format. Expected host:port"})),
        );
    }

    let server_addr = payload.addr.trim().to_string();

    // Remove the server from the list
    if let Some(pos) = servers.iter().position(|s| s.addr == server_addr) {
        servers.remove(pos);
        tracing::info!("Unregistered server: addr={}", server_addr);
        (
            StatusCode::OK,
            Json(serde_json::json!({"message": "Server unregistered successfully"})),
        )
    } else {
        tracing::warn!("Server not found for unregistration: addr={}", server_addr);
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Server not found"})),
        )
    }
}

async fn list_servers(State(state): State<AppState>) -> impl IntoResponse {
    let servers = state.servers.lock().await;

    let server_list_display: Vec<String> = servers
        .iter()
        .map(|server| format!("Model: {}, Addr: {}", server.model_name, server.addr))
        .collect();
    Json(server_list_display)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{self, Request, StatusCode},
        routing::post,
        Router,
    };
    use httptest::{matchers::*, responders::*, Expectation, ServerPool}; // For mock server
    use serde_json::json;
    use tower::ServiceExt; // for `oneshot` and `ready`

    // Helper to create AppState for tests
    fn test_app_state() -> AppState {
        let http_client = Client::builder(TokioExecutor::new()).build_http();
        AppState {
            servers: Arc::new(Mutex::new(vec![])),
            http_client,
        }
    }

    fn test_app(state: AppState) -> Router {
        Router::new()
            .route("/register", post(register_server))
            .route("/unregister", post(unregister_server))
            .route("/list", get(list_servers))
            .fallback(proxy_request_handler) // Keep fallback for proxy testing
            .with_state(state)
    }

    static SERVER_POOL: ServerPool = ServerPool::new(10); // Pool of mock servers for tests

    #[tokio::test]
    async fn test_register_server_ok() {
        let state = test_app_state();
        let app = test_app(state.clone());

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
    async fn test_register_invalid_addr() {
        let state = test_app_state();
        let app = test_app(state.clone());
        let payload = RegisterRequest {
            model_name: "model1".into(),
            addr: "invalid".into(),
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
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_list_servers_empty() {
        let state = test_app_state();
        let app = test_app(state);

        let response = app
            .oneshot(Request::builder().uri("/list").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let server_list: Vec<String> = serde_json::from_slice(&body).unwrap();
        assert!(server_list.is_empty());
    }

    #[tokio::test]
    async fn test_list_servers_with_data() {
        let state = test_app_state();
        state.servers.lock().await.push(ProxyServer {
            model_name: "model1".to_string(),
            addr: "127.0.0.1:9000".to_string(),
        });
        let app = test_app(state);

        let response = app
            .oneshot(Request::builder().uri("/list").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let server_list: Vec<String> = serde_json::from_slice(&body).unwrap();
        assert_eq!(server_list.len(), 1);
        assert_eq!(server_list[0], "Model: model1, Addr: 127.0.0.1:9000");
    }

    #[tokio::test]
    async fn test_proxy_no_servers_registered() {
        let state = test_app_state();
        let app = test_app(state);

        let req_body = json!({
            "model": "test_model",
            "prompt": "Hello"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/v1/completions") // Using a common vLLM path
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_string(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_proxy_model_not_found() {
        let state = test_app_state();
        state.servers.lock().await.push(ProxyServer {
            model_name: "actual_model".to_string(),
            addr: "localhost:8002".to_string(),
        });
        let app = test_app(state);

        let req_body = json!({
            "model": "requested_model_not_found",
            "prompt": "Test"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/v1/chat/completions")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_string(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json_body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json_body["error"],
            "No server registered for model: requested_model_not_found"
        );
    }
    #[tokio::test]
    async fn test_proxy_missing_model_in_body() {
        let state = test_app_state();
        state.servers.lock().await.push(ProxyServer {
            model_name: "some_model".to_string(),
            addr: "localhost:8003".to_string(),
        });
        let app = test_app(state);

        let req_body = json!({ // No "model" field
            "prompt": "Test"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/generate")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_string(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json_body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json_body["error"],
            "Model name is required in the request body"
        );
    }

    #[tokio::test]
    async fn test_proxy_successful_forward_post() {
        let mut mock_server = SERVER_POOL.get_server();
        let model_name = "mocked_model";
        let backend_response_body = json!({"completion": "Mocked response!"});

        mock_server.expect(
            Expectation::matching(all_of![request::method_path("POST", "/v1/completions"),])
                .respond_with(json_encoded(backend_response_body.clone())),
        );

        let state = test_app_state();
        let mock_addr = mock_server.addr().to_string(); // e.g. "127.0.0.1:12345"

        state.servers.lock().await.push(ProxyServer {
            model_name: model_name.to_string(),
            addr: mock_addr.clone(),
        });
        let app = test_app(state);

        let req_body = json!({
            "model": model_name,
            "prompt": "Hello from test"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/v1/completions") // This path will be proxied
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_string(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let actual_response_body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(actual_response_body, backend_response_body);
        mock_server.verify_and_clear(); // Ensure mock was called
    }

    #[tokio::test]
    async fn test_proxy_successful_forward_get_with_query() {
        let mut mock_server = SERVER_POOL.get_server();
        let model_name = "get_model";
        // For GET, model name is in body for this proxy, but vLLM might not expect it
        // This test ensures query params are forwarded.
        // vLLM's /v1/models often doesn't take a body, so we'll adapt the proxy or test
        // what this proxy actually does. This proxy *requires* a model in the body.
        // Let's assume the target also expects a model in body even for GET for this test.

        mock_server.expect(
            Expectation::matching(all_of![
                request::method_path("GET", "/v1/models"),
                request::query(url_decoded(contains(("filter", "active")))),
                // This proxy's current design means model name is still from body
                // request::body_json(json!({"model": model_name}))
            ])
            .respond_with(status_code(200).body("Models list")),
        );

        let state = test_app_state();
        let mock_addr = mock_server.addr().to_string();
        state.servers.lock().await.push(ProxyServer {
            model_name: model_name.to_string(),
            addr: mock_addr.clone(),
        });
        let app = test_app(state);

        // The proxy expects model in body. If target doesn't, this is a mismatch.
        // For this test, we send what the proxy expects.
        let req_body_for_proxy = json!({"model": model_name});

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/v1/models?filter=active")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref()) // Needed for body
                    .body(Body::from(
                        serde_json::to_string(&req_body_for_proxy).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&body_bytes), "Models list");
        mock_server.verify_and_clear();
    }

    #[tokio::test]
    async fn test_proxy_bad_gateway_if_target_server_down() {
        let model_name = "unavailable_model";
        let state = test_app_state();
        // Use an address that is unlikely to be listening
        let unavailable_addr = "127.0.0.1:1".to_string();

        state.servers.lock().await.push(ProxyServer {
            model_name: model_name.to_string(),
            addr: unavailable_addr.clone(),
        });
        let app = test_app(state);

        let req_body = json!({
            "model": model_name,
            "prompt": "This will fail"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/some/path")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_string(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }
}

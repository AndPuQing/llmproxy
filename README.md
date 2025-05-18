# Model Service CLI

![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/AndPuQing/llmproxy/ci.yml?style=flat-square&logo=github)
 ![Crates.io Version](https://img.shields.io/crates/v/llmproxy?style=flat-square&logo=rust)
 ![Crates.io Downloads (recent)](https://img.shields.io/crates/dr/llmproxy?style=flat-square)
[![dependency status](https://deps.rs/repo/github/AndPuQing/llmproxy/status.svg?style=flat-square)](https://deps.rs/repo/github/AndPuQing/llmproxy)
![Crates.io License](https://img.shields.io/crates/l/llmproxy?style=flat-square) ![Crates.io Size](https://img.shields.io/crates/size/llmproxy?style=flat-square)


A command-line interface (CLI) to interact with the Model Service Orchestrator/Proxy. This tool allows you to register, unregister, and list model services managed by the backend server.

## Features

*   **Register:** Register a new model service (e.g., a vLLM instance) with the orchestrator, specifying its model name and address.
*   **Unregister:** Remove a previously registered model service from the orchestrator using its address.
*   **List:** Display all currently registered model services, showing their model names and addresses.

## Prerequisites

1.  **Rust Toolchain:** You need Rust and Cargo installed to build the CLI. Visit [rust-lang.org](https://www.rust-lang.org/tools/install) for installation instructions.
2.  **Running Backend Server:** The [Axum-based Model Service Orchestrator/Proxy](src/bin/llmproxyd) must be running and accessible. By default, this CLI expects the server to be at `http://127.0.0.1:11450`.

## Building

1.  Clone the repository (if you have it in one):
    ```bash
    git clone <your-repo-url>
    cd <repository-name>
    ```
2.  Build the CLI:
    *   For a development build:
        ```bash
        cargo build
        ```
        The executable will be in `./target/debug/llmproxy`.
    *   For a release (optimized) build:
        ```bash
        cargo build --release
        ```
        The executable will be in `./target/release/llmproxy`.

## Usage

The general command structure is:

```bash
./path/to/llmproxy <COMMAND> [OPTIONS]
```

You can get help for the main command or any subcommand:

```bash
./path/to/llmproxy --help
./path/to/llmproxy register --help
```

### Commands

#### 1. `register`

Registers a new model service with the orchestrator.

**Options:**

*   `--model-name <MODEL_NAME>`: The name of the model being served (e.g., "Qwen/Qwen2-7B-Instruct"). (Required)
*   `--addr <ADDR>`: The address (host:port) of the model service (e.g., "localhost:8001"). (Required)

**Example:**

```bash
./target/debug/llmproxy register --model-name "Qwen/Qwen2-7B-Instruct" --addr "127.0.0.1:8001"
```

**Expected Output (Success):**

```
Success (201 Created): Server registered successfully
```
or if already registered:
```
Success (200 OK): Server already registered
```

#### 2. `unregister`

Unregisters an existing model service from the orchestrator using its address.

**Options:**

*   `--addr <ADDR>`: The address (host:port) of the model service to unregister (e.g., "127.0.0.1:8001"). (Required)

**Example:**

```bash
./target/debug/llmproxy unregister --addr "127.0.0.1:8001"
```

**Expected Output (Success):**

```
Success (200 OK): Server unregistered successfully
```
or if not found:
```
Failed (404 Not Found): Server not found
```

#### 3. `list`

Lists all currently registered model services.

**Example:**

```bash
./target/debug/llmproxy list
```

**Expected Output:**

```
Registered model services (2):
  - Model: Qwen/Qwen2-7B-Instruct, Addr: 127.0.0.1:8001
  - Model: Llama3-8B, Addr: 127.0.0.1:8002
```
or if none are registered:
```
No model services registered.
```

## Backend Server

This CLI tool is a client for the Axum-based backend server. Ensure the server is running and configured correctly (defaulting to `http://127.0.0.1:11450`). The server is responsible for:
*   Maintaining the list of active model services.
*   Proxying incoming requests to the appropriate registered model service based on the `model` field in the request body.

```bash
cargo run --release --bin llmproxyd
```

## Troubleshooting

*   **Connection Refused:** Ensure the backend server is running and accessible at `http://127.0.0.1:11450` (or the configured address if you modify the `BASE_URL` in the CLI source).
*   **Unexpected JSON Errors:** Verify that the backend server's API responses match what the CLI expects.
*   **`Failed to parse server response`:** This could indicate an issue with the server's response format or a network problem. The CLI will attempt to print the raw body which might give clues.


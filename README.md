# Model Service CLI

![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/AndPuQing/llmproxy/ci.yml?style=flat-square&logo=github)
 ![Crates.io Version](https://img.shields.io/crates/v/llmproxy?style=flat-square&logo=rust)
 ![Crates.io Downloads (recent)](https://img.shields.io/crates/dr/llmproxy?style=flat-square)
[![dependency status](https://deps.rs/repo/github/AndPuQing/llmproxy/status.svg?style=flat-square)](https://deps.rs/repo/github/AndPuQing/llmproxy)
![Crates.io License](https://img.shields.io/crates/l/llmproxy?style=flat-square) ![Crates.io Size](https://img.shields.io/crates/size/llmproxy?style=flat-square)


A command-line interface (CLI) to interact with the Model Service Orchestrator/Proxy. This tool allows you to register, unregister, and list model services managed by the backend server.

## Features

*   **Register:** Register a new model service (e.g., a vLLM instance) with the orchestrator, specifying its model name and address.
*   **Unregister:** Remove a previously registered model service from the orchestrator using its index number or address.
*   **List:** Display all currently registered model services in a clean table format with index numbers for easy reference.

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
✔ Registered meta-llama/Llama-2-7b-chat-hf at 127.0.0.1:8001
```

#### 2. `unregister`

Unregisters an existing model service from the orchestrator using its index number or address.

**Arguments:**

*   `<TARGET>`: Service index (e.g., 1, 2, 3) or address (e.g., localhost:8001). (Required)

**Examples:**

```bash
# Unregister by index number (most convenient)
./target/debug/llmproxy unregister 1

# Unregister by address (backward compatible)
./target/debug/llmproxy unregister "127.0.0.1:8001"
```

**Expected Output (Success):**

```
✔ Unregistered service #1 (127.0.0.1:8001)
```
or:
```
✔ Unregistered service at 127.0.0.1:8001
```

**Error Examples:**
```
✖ Index 5 not found. Only 2 services are registered.
```

#### 3. `list`

Lists all currently registered model services in a clean table format with index numbers.

**Example:**

```bash
./target/debug/llmproxy list
```

**Expected Output:**

```
✔ 2 registered services

Label  Model                          Address
#1     meta-llama/Llama-2-7b-chat-hf  10.150.10.75:18012
#2     Qwen/Qwen2-7B-Instruct         127.0.0.1:8001

💡 You can unregister services by index or address:
  → llmproxy unregister 1
  → llmproxy unregister "localhost:8001"
```

**When no services are registered:**

```
ℹ No model services are currently registered
  → Use llmproxy register --model-name <MODEL> --addr <ADDRESS> to register a new service
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
    ```
    ✖ Cannot connect to llmproxyd server
      → Make sure the server is running on http://127.0.0.1:11450
      → Start it with: llmproxyd
    ```
*   **Invalid Index:** When using numeric indices, ensure they are within the valid range:
    ```
    ✖ Index 5 not found. Only 2 services are registered.
    ```
*   **Server Errors:** The CLI provides clear error messages with context and suggestions for resolution.


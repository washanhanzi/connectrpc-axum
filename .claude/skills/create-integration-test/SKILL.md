---
name: create-integration-test
description: Create integration tests for connectrpc-axum. Use when the user wants to add a new Rust client test or Rust server test for the integration test suite.
---

# create-integration-test

Create integration tests for connectrpc-axum. This skill generates either:
- **Rust client tests** - Test Rust clients against servers
- **Rust server tests** - New server binaries tested by Go/Rust clients

## Usage

When the user wants to create a new integration test, ask which type:
1. **Rust Client Test** - Creates a new client binary in `connectrpc-axum-examples/src/bin/client/`
2. **Rust Server Test** - Creates a new server binary in `connectrpc-axum-examples/src/bin/`

## Creating a Rust Client Test

### Location
`connectrpc-axum-examples/src/bin/client/<name>-client.rs`

### Template Structure

```rust
//! <Description of what this client tests>
//!
//! Tests the connectrpc-axum-client against the <server type>.
//!
//! Usage:
//!   # First, start a server in another terminal:
//!   cargo run --bin <server-bin> --no-default-features
//!
//!   # Then run the client test (defaults to http://localhost:3000):
//!   cargo run --bin <name>-client --no-default-features
//!
//!   # Or specify a custom server URL:
//!   cargo run --bin <name>-client --no-default-features -- http://localhost:8080

use connectrpc_axum_client::{ConnectClient, ConnectResponse as ClientResponse};
use connectrpc_axum_examples::{/* Request/Response types */};
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Check command line args first, then SERVER_URL env var, then default
    let base_url = env::args().nth(1).or_else(|| env::var("SERVER_URL").ok())
        .unwrap_or_else(|| "http://localhost:3000".to_string());

    println!("=== <Test Name> ===");
    println!("Server URL: {}", base_url);
    println!();

    // Test cases here...
    // Use assert!() for validations
    // Print PASS/FAIL for each test

    println!();
    println!("=== All tests passed! ===");

    Ok(())
}
```

### Client Test Patterns

#### Unary Call (JSON)
```rust
let client = ConnectClient::builder(&base_url).use_json().build()?;
let request = HelloRequest { name: Some("Alice".to_string()), ... };
let response: ClientResponse<HelloResponse> = client
    .call_unary("hello.HelloWorldService/SayHello", &request)
    .await?;
assert_eq!(response.message, "Hello, Alice!");
```

#### Unary Call (Proto)
```rust
let client = ConnectClient::builder(&base_url).use_proto().build()?;
// Same as JSON but uses protobuf encoding
```

#### Server Streaming
```rust
use futures::StreamExt;

let client = ConnectClient::builder(&base_url).use_json().build()?;
let response = client
    .call_server_stream::<HelloRequest, HelloResponse>(
        "hello.HelloWorldService/SayHelloStream",
        &request,
    )
    .await?;

let mut stream = response.into_inner();
while let Some(result) = stream.next().await {
    let msg = result?;
    println!("Received: {}", msg.message);
}
```

#### Client Streaming
```rust
use futures::stream;

let client = ConnectClient::builder(&base_url).use_json().build()?;
let messages = vec![/* request messages */];
let request_stream = stream::iter(messages);

let response = client
    .call_client_stream::<EchoRequest, EchoResponse, _>(
        "echo.EchoService/EchoClientStream",
        request_stream,
    )
    .await?;
```

#### Bidirectional Streaming
```rust
use futures::{StreamExt, stream};

let client = ConnectClient::builder(&base_url).use_json().build()?;
let messages = vec![/* request messages */];
let request_stream = stream::iter(messages);

let response = client
    .call_bidi_stream::<EchoRequest, EchoResponse, _>(
        "echo.EchoService/EchoBidiStream",
        request_stream,
    )
    .await?;

let mut stream = response.into_inner();
while let Some(result) = stream.next().await {
    let msg = result?;
    println!("Got: {}", msg.message);
}
```

#### With Custom Headers/Timeout
```rust
use connectrpc_axum_client::CallOptions;
use std::time::Duration;

let options = CallOptions::new()
    .timeout(Duration::from_secs(10))
    .header("x-custom-header", "value")
    .header("authorization", "Bearer token");

let response = client
    .call_unary_with_options("service/Method", &request, options)
    .await?;
```

#### Error Handling
```rust
use connectrpc_axum_client::ClientError;

let result: Result<ClientResponse<HelloResponse>, ClientError> = client
    .call_unary("service/Method", &request)
    .await;

match result {
    Err(ClientError::Transport(_)) => println!("PASS: Got Transport error"),
    Err(ClientError::Connect(e)) => println!("Connect error: {:?}", e.code()),
    Ok(_) => println!("Success"),
}
```

### Register in Integration Test Runner

After creating the client, add it to `integration-test.rs`:

```rust
// In get_rust_client_tests()
RustClientTest {
    name: "Rust Client: <Name>",
    server: ServerConfig { name: "<server-bin>", features: None }, // or Some("tonic")
    client_bin: "<name>-client",
},
```

## Creating a Rust Server Test

### Location
`connectrpc-axum-examples/src/bin/<name>.rs`

### Template Structure (Connect-only)

```rust
//! <Description of the server example>
//!
//! Run with: cargo run --bin <name> --no-default-features
//! Test with Go client: go run ./cmd/client --protocol connect <test>

use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{/* types */, helloworldservice};

/// Handler for the RPC method
async fn handler(
    ConnectRequest(req): ConnectRequest<RequestType>,
) -> Result<ConnectResponse<ResponseType>, ConnectError> {
    // Implementation
    Ok(ConnectResponse::new(ResponseType { ... }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let router = helloworldservice::HelloWorldServiceBuilder::new()
        .method_name(handler)
        .build_connect();

    let addr = connectrpc_axum_examples::server_addr();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== <Example Name> ===");
    println!("Server listening on http://{}", addr);
    // ... print test instructions

    axum::serve(listener, router).await?;
    Ok(())
}
```

### Template Structure (Connect + gRPC with Tonic)

```rust
//! <Description>
//!
//! Run with: cargo run --bin <name> --features tonic

use axum::extract::State;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{/* types */, helloworldservice};
use std::sync::Arc;

#[derive(Clone, Default)]
struct AppState {
    // Shared state
}

async fn handler(
    State(state): State<AppState>,
    ConnectRequest(req): ConnectRequest<RequestType>,
) -> Result<ConnectResponse<ResponseType>, ConnectError> {
    // Implementation
    Ok(ConnectResponse::new(ResponseType { ... }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app_state = AppState::default();

    let (connect_router, grpc_server) =
        helloworldservice::HelloWorldServiceTonicCompatibleBuilder::new()
            .method_name(handler)
            .with_state(app_state)
            .build();

    let dispatch = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(connect_router)
        .add_grpc_service(grpc_server)
        .build();

    let addr = connectrpc_axum_examples::server_addr();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== <Example Name> ===");
    // ...

    let make = tower::make::Shared::new(dispatch);
    axum::serve(listener, make).await?;
    Ok(())
}
```

### Server Handler Patterns

#### Unary Handler
```rust
async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let name = req.name.unwrap_or_else(|| "World".to_string());
    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello, {}!", name),
        response_type: None,
    }))
}
```

#### Server Streaming Handler
```rust
async fn say_hello_stream(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectServerStream<HelloResponse>, ConnectError> {
    let name = req.name.unwrap_or_else(|| "World".to_string());

    let stream = async_stream::stream! {
        yield Ok(HelloResponse { message: format!("Hello, {}!", name), response_type: None });
        yield Ok(HelloResponse { message: "Goodbye!".to_string(), response_type: None });
    };

    Ok(ConnectServerStream::new(stream))
}
```

#### Client Streaming Handler
```rust
async fn echo_client_stream(
    ConnectClientStream(mut stream): ConnectClientStream<EchoRequest>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    let mut messages = Vec::new();
    while let Some(result) = stream.next().await {
        let req = result?;
        messages.push(req.message);
    }

    Ok(ConnectResponse::new(EchoResponse {
        message: format!("Received {} messages", messages.len()),
    }))
}
```

#### Bidi Streaming Handler
```rust
async fn echo_bidi_stream(
    ConnectBidiStream(mut stream): ConnectBidiStream<EchoRequest>,
) -> Result<ConnectServerStream<EchoResponse>, ConnectError> {
    let output = async_stream::stream! {
        while let Some(result) = stream.next().await {
            match result {
                Ok(req) => {
                    yield Ok(EchoResponse {
                        message: format!("Echo: {}", req.message),
                    });
                }
                Err(e) => {
                    yield Err(e);
                    break;
                }
            }
        }
    };

    Ok(ConnectServerStream::new(output))
}
```

#### With Axum Extractors
```rust
use axum::extract::{State, Extension};
use axum::http::HeaderMap;

async fn handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectRequest(req): ConnectRequest<RequestType>,
) -> Result<ConnectResponse<ResponseType>, ConnectError> {
    // Access headers, state, etc.
    if let Some(auth) = headers.get("authorization") {
        // ...
    }
    Ok(ConnectResponse::new(ResponseType { ... }))
}
```

#### Returning Errors
```rust
use connectrpc_axum::Code;

async fn handler(
    ConnectRequest(req): ConnectRequest<RequestType>,
) -> Result<ConnectResponse<ResponseType>, ConnectError> {
    if req.name.is_none() {
        return Err(ConnectError::new(Code::InvalidArgument, "name is required"));
    }
    // ...
}
```

### Register in Integration Test Runner

After creating the server, add a test in `integration-test.rs`:

```rust
// In get_go_tests()
TestConfig {
    name: "Go: <TestName>",
    server: ServerConfig { name: "<server-bin>", features: None }, // or Some("tonic")
    go_test_pattern: "Test<Name>",
},
```

And create the corresponding Go test in `connectrpc-axum-examples/go-client/client_test.go`.

## Available Proto Types

From `connectrpc-axum-examples/src/lib.rs`:

### HelloWorldService
- `HelloRequest` - `name: Option<String>`, `hobbies: Vec<String>`, `greeting_type: Option<i32>`
- `HelloResponse` - `message: String`, `response_type: Option<i32>`

### EchoService
- `EchoRequest` - `message: String`
- `EchoResponse` - `message: String`

## Cargo.toml Binary Entry

Add to `connectrpc-axum-examples/Cargo.toml`:

```toml
[[bin]]
name = "<bin-name>"
path = "src/bin/<path>.rs"
```

For clients in the `client/` subdirectory:
```toml
[[bin]]
name = "<name>-client"
path = "src/bin/client/<name>-client.rs"
```

## Checklist

When creating a new integration test:

1. [ ] Create the binary file (client or server)
2. [ ] Add `[[bin]]` entry to `Cargo.toml`
3. [ ] Register in `integration-test.rs` (appropriate function)
4. [ ] If server test, create matching Go test in `go-client/client_test.go`
5. [ ] Run `cargo make test` to verify

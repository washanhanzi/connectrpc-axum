# ConnectRPC Axum

Brings the protobuf-based [Connect RPC framework](https://connect.build/docs/introduction) to Rust via idiomatic [Axum](https://github.com/tokio-rs/axum).

This crate provides a set of tools to build Connect-compliant RPC services that
feel idiomatic to Axum developers. It uses standard Axum extractors, response
types, and a compile-time route generator to integrate seamlessly into existing
Axum applications.

# Features 🔍

- **Compile-time Route Generation:** `axum-connect-build` generates an `routes()` function from your `.proto` files, ensuring your routes are always in sync with your service definition.
- **Axum-native:** Handlers are standard `async fn` that use `axum::extract::FromRequest` and `axum::response::IntoResponse`.
- **Unary and Streaming:** Supports both unary and server-streaming RPCs.
- **Error Handling:** Provides a `ConnectError` type that automatically maps to
  Connect-compliant error responses.

# Getting Started 🤓

_Prior knowledge with [Protobuf](https://github.com/protocolbuffers/protobuf) (both the IDL and its use in RPC frameworks) and [Axum](https://github.com/tokio-rs/axum) are assumed._

## Dependencies 👀

You'll need `connectrpc-axum` for the runtime and `connectrpc-axum-build` for code generation.

```sh
cargo add connectrpc-axum axum tokio --features=tokio/full
cargo add --build connectrpc-axum-build prost-build
```

## Protobuf File 🥱

`proto/hello.proto`
```protobuf
syntax = "proto3";

package hello;

message HelloRequest { string name = 1; }
message HelloResponse { string message = 1; }

service HelloWorldService {
  rpc SayHello(HelloRequest) returns (HelloResponse) {}
  rpc SayHelloStream(HelloRequest) returns (stream HelloResponse) {}
}
```

## Code Generation 🤔

Use `connectrpc-axum-build` in your `build.rs` to generate the routing function.

`build.rs`
```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = prost_build::Config::new();
    config.type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]");
    config.type_attribute(".", "#[serde(rename_all = \"camelCase\")]");

    connectrpc_axum_build::compile(
        &["proto/hello.proto"],
        &["proto/"],
        config
    )?;
    Ok(())
}
```

This will generate a `helloworldservice::routes()` function in your `hello.rs` file.

## The Fun Part 😁

Now, implement your service using standard Axum handlers and the generated `routes()` function.

`src/main.rs`
```rust
use axum::extract::State;
use connectrpc_axum::{
    connect_core::Code,
    prelude::*,
};
use futures::{stream, Stream};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tower_http::cors::CorsLayer;

pub mod hello {
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
}
use hello::{helloworldservice, HelloRequest, HelloResponse};

#[derive(Clone, Default)]
struct AppState {
    counter: Arc<AtomicUsize>,
}

#[tokio::main]
async fn main() {
    let app = axum::Router::new()
        .merge(helloworldservice::routes(
            say_hello_unary,
            say_hello_stream,
        ))
        .fallback(unimplemented)
        .with_state(AppState::default())
        .layer(CorsLayer::very_permissive());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3030")
        .await
        .unwrap();
    println!("listening on http://{:?}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

async fn say_hello_unary(
    State(state): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let count = state.counter.fetch_add(1, Ordering::SeqCst);
    println!("Unary request #{}: {:?}", count, req);
    Ok(ConnectResponse(HelloResponse {
        message: format!("Hello, {}!", req.name),
    }))
}

async fn say_hello_stream(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> ConnectStreamResponse<impl Stream<Item = Result<HelloResponse, ConnectError>>> {
    let stream = stream::iter(vec![
        Ok(HelloResponse { message: format!("Hello, {}", req.name) }),
        Ok(HelloResponse { message: "Here is a second message.".to_string() }),
        Err(ConnectError::new(Code::Unknown, "Stream error!".to_string())),
    ]);
    ConnectStreamResponse::new(stream)
}

async fn unimplemented() -> ConnectError {
    ConnectError::new(
        Code::Unimplemented,
        "The requested service has not been implemented.",
    )
}
```

## SEND IT 🚀

```sh
cargo run -p connectrpc-axum-examples
```

You can test your service using a Connect-compatible client like [Buf Studio](https://buf.build/studio) or `curl`.

**Unary Request:**
```sh
curl -X POST http://localhost:3030/hello.HelloWorldService/SayHello \
     -H "Content-Type: application/json" \
     -d '{"name":"Axum"}'
```
> `{"message":"Hello, Axum!"}`

**Streaming Request:**
```sh
curl -X POST http://localhost:3030/hello.HelloWorldService/SayHelloStream \
     -H "Content-Type: application/json" \
     -d '{"name":"Stream"}'
```
> The output will be a stream of JSON messages, framed according to the Connect protocol.

# License 🧾

ConnectRPC-Axum is dual licensed (at your option)

- MIT License ([LICENSE-MIT](LICENSE-MIT) or [http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or [http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0))

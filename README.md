# ConnectRPC Axum

[![connectrpc-axum](https://img.shields.io/crates/v/connectrpc-axum.svg)](https://crates.io/crates/connectrpc-axum)
[![connectrpc-axum-build](https://img.shields.io/crates/v/connectrpc-axum-build.svg)](https://crates.io/crates/connectrpc-axum-build)
[![Documentation](https://docs.rs/connectrpc-axum/badge.svg)](https://docs.rs/connectrpc-axum)
[![License](https://img.shields.io/crates/l/connectrpc-axum.svg)](LICENSE)

A Rust library that brings [ConnectRPC](https://connectrpc.com/) protocol support to the [Axum](https://github.com/tokio-rs/axum) web framework, with optional [Tonic](https://github.com/hyperium/tonic) integration for serving gRPC on the same port.

> **Status**: Under active development. Not recommended for production use yet.

## Features

| Protocol | Support |
|----------|---------|
| Connect (JSON/Proto) | Native |
| gRPC | Via Tonic integration |
| gRPC-Web | Via tonic-web layer |

- Type-safe handlers generated from Protocol Buffers
- Full Axum ecosystem support (extractors, middleware, state)
- Automatic content negotiation (JSON/binary protobuf)
- All protocols served on the same port

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
connectrpc-axum = "*"
axum = "0.8"
prost = "0.14"
pbjson = "0.8"
tokio = { version = "1", features = ["full"] }

[build-dependencies]
connectrpc-axum-build = "*"
```

Create `build.rs`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto").compile()?;
    Ok(())
}
```

Define handlers:

```rust
use connectrpc_axum::prelude::*;

async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse {
        message: format!("Hello, {}!", req.name.unwrap_or_default()),
    }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = helloworldservice::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .build_connect();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

## Documentation

See the [full documentation](https://phlx-io.github.io/connectrpc-axum/) for:

- [Getting Started](https://phlx-io.github.io/connectrpc-axum/guide/)
- [MakeServiceBuilder](https://phlx-io.github.io/connectrpc-axum/guide/configuration)
- [HTTP Endpoints](https://phlx-io.github.io/connectrpc-axum/guide/http-endpoints)
- [Tonic gRPC Integration](https://phlx-io.github.io/connectrpc-axum/guide/tonic)
- [build.rs Configuration](https://phlx-io.github.io/connectrpc-axum/guide/build)

## Development

### Claude Code Slash Commands

This project provides [slash commands](https://docs.anthropic.com/en/docs/claude-code/slash-commands) for common development tasks:

| Command | Description |
|---------|-------------|
| `/connectrpc-axum:submit-issue` | Report bugs, request features, or ask questions |
| `/connectrpc-axum:test` | Run the full test suite |

Usage:

```bash
claude /connectrpc-axum:submit-issue "Description of your issue or feature request"
claude /connectrpc-axum:test
```

If not using Claude Code, see the corresponding skill files in [`.claude/skills/`](.claude/skills/) for instructions.

### Architecture

See [`.claude/architecture.md`](.claude/architecture.md) for detailed documentation on the project structure, core modules, and design decisions.

## Examples

See [connectrpc-axum-examples](./connectrpc-axum-examples) for complete working examples:

```bash
cd connectrpc-axum-examples
cargo run --bin connect-unary
```

## Acknowledgments

This project started as a fork of [AThilenius/axum-connect](https://github.com/AThilenius/axum-connect).

## Learn More

- [ConnectRPC Protocol](https://connectrpc.com/docs/protocol/)
- [Axum Documentation](https://docs.rs/axum/)
- [Tonic gRPC](https://docs.rs/tonic/)

## License

MIT License - see [LICENSE](LICENSE) for details.

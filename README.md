# ConnectRPC Axum

[![connectrpc-axum](https://img.shields.io/crates/v/connectrpc-axum?label=connectrpc-axum)](https://crates.io/crates/connectrpc-axum)
[![connectrpc-axum-build](https://img.shields.io/crates/v/connectrpc-axum-build?label=connectrpc-axum-build)](https://crates.io/crates/connectrpc-axum-build)
[![connectrpc-axum-client](https://img.shields.io/crates/v/connectrpc-axum-client?label=connectrpc-axum-client)](https://crates.io/crates/connectrpc-axum-client)
[![connectrpc-axum-core](https://img.shields.io/crates/v/connectrpc-axum-core?label=connectrpc-axum-core)](https://crates.io/crates/connectrpc-axum-core)
[![Documentation](https://docs.rs/connectrpc-axum/badge.svg)](https://docs.rs/connectrpc-axum)
[![License](https://img.shields.io/crates/l/connectrpc-axum.svg)](LICENSE)

A Rust library that brings [ConnectRPC](https://connectrpc.com/) protocol support to the [Axum](https://github.com/tokio-rs/axum) web framework, with optional [Tonic](https://github.com/hyperium/tonic) integration for serving gRPC or gRPC-Web on the same port.

> **Status**: Under active development. Not recommended for production use yet.

📝 [The Origin of connectrpc-axum](https://washanhanzi.github.io/connectrpc-axum/blog/origin) - Learn about the problems this library solves and the design decisions behind it.

## Features

| Protocol | Support |
|----------|---------|
| Connect (JSON/Proto) | Native (server + client) |
| gRPC | Via Tonic integration |
| gRPC-Web | Via tonic-web layer |

### Server
- Type-safe handlers generated from Protocol Buffers
- Full Axum ecosystem support (extractors, middleware, state)
- Automatic content negotiation (JSON/binary protobuf)
- All protocols served on the same port

### Client
- Type-safe RPC client generated from Protocol Buffers
- All RPC patterns: unary, server streaming, client streaming, bidirectional
- JSON and protobuf encoding support
- Request compression (gzip, brotli, zstd)
- Middleware support via interceptors

## 📖 [Documentation](https://washanhanzi.github.io/connectrpc-axum/guide)

## Development

### Claude Code Skills

This project includes [Claude Code skills](https://docs.anthropic.com/en/docs/claude-code/skills) to assist with development. See the skill files in [`.claude/skills/`](.claude/skills/) for details.

| Skill | Description |
|-------|-------------|
| `submit-issue` | Report bugs, request features, or ask questions |
| `resolve-issue` | Investigate and resolve GitHub issues |
| `test` | Run the full test suite |

### Architecture

See [`architecture.md`](./docs/guide/architecture.md) for detailed documentation on the project structure, core modules, and design decisions.

## Examples

See [connectrpc-axum-examples](./connectrpc-axum-examples) for complete working examples.

## Acknowledgments

This project started as a fork of [AThilenius/axum-connect](https://github.com/AThilenius/axum-connect).

## Learn More

- [ConnectRPC Protocol](https://connectrpc.com/docs/protocol/)
- [Axum Documentation](https://docs.rs/axum/)
- [Tonic gRPC](https://docs.rs/tonic/)

## License

MIT License - see [LICENSE](LICENSE) for details.

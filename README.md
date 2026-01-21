# ConnectRPC Axum

[![connectrpc-axum](https://img.shields.io/crates/v/connectrpc-axum.svg)](https://crates.io/crates/connectrpc-axum)
[![connectrpc-axum-build](https://img.shields.io/crates/v/connectrpc-axum-build.svg)](https://crates.io/crates/connectrpc-axum-build)
[![Documentation](https://docs.rs/connectrpc-axum/badge.svg)](https://docs.rs/connectrpc-axum)
[![License](https://img.shields.io/crates/l/connectrpc-axum.svg)](LICENSE)

A Rust library that brings [ConnectRPC](https://connectrpc.com/) protocol support to the [Axum](https://github.com/tokio-rs/axum) web framework, with optional [Tonic](https://github.com/hyperium/tonic) integration for serving gRPC or gRPC-Web on the same port.

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

See [`/architecture.md`](./docs/guide/architecture.md) for detailed documentation on the project structure, core modules, and design decisions.

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

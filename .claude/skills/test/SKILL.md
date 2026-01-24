---
name: test
description: Run the complete test suite for connectrpc-axum. Use when the user asks to run tests, verify changes, or check if the code works.
---

# test

Run the complete test suite for connectrpc-axum using cargo-make.

## Quick Start

```bash
# Run all tests (unit + Rust client + Go client)
cargo make test
```

This runs the integration test runner which:
- Allocates unique ports dynamically for each test (prevents port conflicts)
- Runs unit tests, Rust client tests, and Go client tests
- Provides colored output with pass/fail status
- Ensures proper cleanup of server processes

## Cargo Make Commands

| Command | Description |
|---------|-------------|
| `cargo make test` | Run all tests |
| `cargo make test-unit` | Run unit tests only |
| `cargo make test-rust-client` | Run Rust client integration tests (against Rust servers) |
| `cargo make test-go-client` | Run Go client integration tests (against Rust servers) |
| `cargo make test-cross-impl` | Run cross-implementation tests (Rust clients against Go server) |
| `cargo make test-verbose` | Run all tests with verbose output |
| `cargo make test-filter <pattern>` | Run tests matching a filter |

### Examples

```bash
# Run all tests (unit + Rust client + Go client + cross-impl)
cargo make test

# Run only unit tests (fastest)
cargo make test-unit

# Run only Rust client tests (against Rust servers)
cargo make test-rust-client

# Run only Go client tests (against Rust servers)
cargo make test-go-client

# Run only cross-implementation tests (Rust clients against Go server)
cargo make test-cross-impl

# Run with verbose output
cargo make test-verbose

# Filter tests by name
cargo make test-filter TestConnectUnary
cargo make test-filter "Unary"
```

## CI Commands

```bash
# Full CI: fmt-check + clippy + all tests
cargo make ci

# Quick CI: fmt-check + clippy + unit tests only
cargo make ci-quick
```

## Direct Integration Test Runner (Alternative)

If cargo-make is not installed, run the integration test binary directly:

```bash
# Run all tests
cargo run -p connectrpc-axum-examples --bin integration-test

# Run only unit tests
cargo run -p connectrpc-axum-examples --bin integration-test -- --unit

# Run only Rust client tests
cargo run -p connectrpc-axum-examples --bin integration-test -- --rust-client

# Run only Go client tests
cargo run -p connectrpc-axum-examples --bin integration-test -- --go-client

# Filter tests by name
cargo run -p connectrpc-axum-examples --bin integration-test -- --filter TestConnectUnary

# Verbose output
cargo run -p connectrpc-axum-examples --bin integration-test -- -v
```

## Success Criteria

The test runner shows a summary:

```
=== Summary ===

Passed: X
Failed: 0
Total: X
```

All tests should pass (Failed: 0).

## Test Categories

### Unit Tests
- All crate unit tests and doc tests via `cargo test --workspace`

### Rust Client Tests (against Rust servers)
| Test | Server | Description |
|------|--------|-------------|
| unary-client | connect-unary | Unary calls (JSON + Proto encoding) |
| server-stream-client | connect-server-stream | Server streaming |
| client-stream-client | connect-client-stream | Client streaming |
| bidi-stream-client | connect-bidi-stream | Bidirectional streaming |

### Go Client Tests (against Rust servers)
| Test | Server | Protocol |
|------|--------|----------|
| TestConnectUnary | connect-unary | Connect |
| TestConnectServerStream | connect-server-stream | Connect |
| TestTonicUnary* | tonic-unary | Connect + gRPC |
| TestTonicServerStream* | tonic-server-stream | Connect + gRPC |
| TestTonicBidiStream* | tonic-bidi-stream | Connect + gRPC |
| TestGRPCWeb | grpc-web | gRPC-Web |
| TestProtocolVersion | protocol-version | Connect |
| TestTimeout | timeout | Connect |
| TestExtractor* | extractor-* | Connect |
| TestStreamingErrorHandling | streaming-error-repro | Connect |

### Cross-Implementation Tests (Rust clients against Go server)
| Rust Client | Description |
|-------------|-------------|
| unary-client | Unary calls (JSON + Proto encoding) |
| server-stream-client | Server streaming |
| client-stream-client | Client streaming |
| bidi-stream-client | Bidirectional streaming |
| typed-client | Typed client API |

The Go server is located in `connectrpc-axum-examples/go-server/` and implements all proto services (HelloWorldService, EchoService) using connect-go.

## Failure Handling

**Test runner failures**: Check the failed test name and output shown below it.

**Port issues**: The integration test runner uses dynamic ports, so port conflicts should not occur.

**Build errors**: Run `cargo make build-examples` or `cargo build -p connectrpc-axum-examples --features tonic` first.

**Go dependency issues**: Run `go mod tidy` in `connectrpc-axum-examples/go-client/`.

**cargo-make not installed**: Install with `cargo install cargo-make` or use the direct integration test runner commands above.

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `PORT` | Server listen port | 3000 |
| `SERVER_URL` | Client target URL | http://localhost:3000 |

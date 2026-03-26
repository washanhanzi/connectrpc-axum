---
name: test
description: Run the complete test suite for connectrpc-axum. Use when the user asks to run tests, verify changes, or check if the code works.
---

# test

Run the cross-implementation protocol test suite (`connectrpc-axum-test`).

## Quick Start

```bash
cargo make test
```

## How It Works

Uses Unix domain sockets (Linux abstract sockets or file-based). Each test scenario runs against **all 4 client/server combinations** concurrently:

- Rust client against Rust server
- Rust client against Go server
- Go client against Rust server
- Go client against Go server

## Architecture

Each test scenario follows this pattern:

1. **Orchestrator** (`src/<test_name>.rs`): Builds Go binaries, spawns Rust + Go servers, runs all 4 client combos concurrently, reports results.
2. **Rust server** (`src/<test_name>/server.rs`): Implements the service under test using `connectrpc-axum`.
3. **Rust client** (`src/<test_name>/client.rs`): Raw HTTP client test cases using `hyper` over Unix sockets.
4. **Go server** (`go/<test_name>/server/server.go`): Implements the same service using `connect-go`.
5. **Go client** (`go/<test_name>/client/client.go`): Go HTTP client test cases over Unix sockets.

## Source Files

```
connectrpc-axum-test/
├── src/
│   ├── main.rs                          # Entry point, creates TestSockets, calls orchestrators
│   ├── socket.rs                        # Unix socket abstraction (abstract on Linux, file-based otherwise)
│   ├── server_timeout.rs                # Orchestrator example
│   └── server_timeout/
│       ├── server.rs                    # Rust server
│       └── client.rs                    # Rust client
├── go/
│   └── server_timeout/
│       ├── server/server.go             # Go server
│       └── client/client.go             # Go client
├── proto/
│   ├── hello.proto                      # HelloWorldService
│   └── echo.proto                       # EchoService
├── build.rs                             # Proto compilation via connectrpc-axum-build
├── Cargo.toml
├── buf.yaml
├── buf.gen.yaml
└── integration-tests.feature            # BDD spec for all test scenarios
```

## Current Test Scenarios (32 total)

| Scenario | Description |
|----------|-------------|
| server_timeout | Connect-Timeout-Ms header enforcement |
| connect_unary | Basic unary request/response |
| connect_server_stream | Server streaming |
| connect_client_stream | Client streaming |
| connect_bidi_stream | Bidirectional streaming |
| error_details | Structured error details |
| protocol_version | Connect-Protocol-Version validation |
| streaming_error | Streaming errors in EndStream frame |
| send_max_bytes | Unary send size limit |
| receive_max_bytes | Unary receive size limit (64 bytes) |
| receive_max_bytes_5mb | Unary receive size limit (5MB) |
| receive_max_bytes_unlimited | No receive size limit |
| streaming_send_max_bytes | Streaming send size limit |
| streaming_receive_max_bytes | Streaming receive size limit |
| get_request | HTTP GET for idempotent methods |
| unary_error_metadata | Custom metadata on error responses |
| endstream_metadata | Metadata in EndStream frames |
| extractor_connect_error | Custom extractor with ConnectError rejection |
| extractor_http_response | Custom extractor with plain HTTP rejection |
| streaming_extractor | Server stream extractor (x-api-key) |
| streaming_extractor_client | Client stream extractor (x-api-key) |
| protocol_negotiation | Unsupported content-type returns 415 |
| axum_router | Plain axum routes alongside Connect RPC |
| streaming_compression_gzip | Gzip compression on server streams |
| client_streaming_compression | Gzip compression on client streams |
| compression_algos | Deflate, brotli, zstd compression (Rust server only) |
| tonic_unary | Tonic interop: unary via Connect + gRPC |
| tonic_server_stream | Tonic interop: server streaming via Connect + gRPC |
| tonic_bidi_server | Tonic interop: bidi + client streaming via gRPC |
| grpc_web | gRPC-Web protocol support |
| tonic_extractor | Tonic extractor across Connect + gRPC |
| idempotency_get_connect_client | connect-go client HTTP GET for idempotent methods |

## Adding a New Test Scenario

Follow the `server_timeout` pattern:

1. Create orchestrator: `src/<test_name>.rs` (build Go binaries, spawn servers, run clients)
2. Create Rust server: `src/<test_name>/server.rs`
3. Create Rust client: `src/<test_name>/client.rs` with `TestCase` structs
4. Create Go server: `go/<test_name>/server/server.go`
5. Create Go client: `go/<test_name>/client/client.go`
6. Register in `src/main.rs`: add module declaration and call `<test_name>::run()`
7. Document in `integration-tests.feature`

## Output Format

```
Building Go binaries...
=== Timeout Integration Tests ===
  PASS  Rust Server + Go Client
  PASS  Go Server + Go Client
  PASS  Rust Server + Rust Client / short timeout fails
  PASS  Rust Server + Rust Client / long timeout succeeds
  PASS  Rust Server + Rust Client / no timeout succeeds
  PASS  Go Server + Rust Client / short timeout fails
  PASS  Go Server + Rust Client / long timeout succeeds
  PASS  Go Server + Rust Client / no timeout succeeds

8/8 passed
```

## CI Commands

```bash
# Full CI: fmt-check + clippy + all tests
cargo make ci

# Quick CI: fmt-check + clippy only
cargo make ci-quick
```

## Success Criteria

All scenarios should show `X/X passed` (zero failures).

## Failure Handling

**Test failures**: Check the failed test name and output shown below it.

**Build errors**: Run `cargo build -p connectrpc-axum-test` first.

**Go dependency issues**: Run `go mod tidy` in `connectrpc-axum-test/go/`.

**cargo-make not installed**: Install with `cargo install cargo-make` or run directly with `cargo run -p connectrpc-axum-test`.

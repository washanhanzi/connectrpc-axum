---
name: test
description: Run the complete test suite for connectrpc-axum. Use when the user asks to run tests, verify changes, or check if the code works.
---

# test

Run the complete test suite for connectrpc-axum: unit tests, doc tests, Rust client integration tests, and Go client integration tests.

## Instructions

Run all test suites in order:

### 1. Unit Tests

```bash
cargo test
```

### 2. Rust Client Integration Tests

Tests the `connectrpc-axum-client` crate against the Rust server.

**Location**: `connectrpc-axum-examples/src/bin/client/` (client binaries are separate from server binaries in `src/bin/`)

```bash
# Start the server in background, run client test, then stop server
cargo run --bin connect-unary --no-default-features &
sleep 1
cargo run --bin unary-client --no-default-features
kill %1 2>/dev/null || true
```

The Rust client tests verify:
- JSON and Proto encoding
- Response wrapper methods (Deref, map, into_parts)
- Metadata extraction
- Connection error handling

### 3. Go Client Integration Tests

Run from the repo root (use `-C` to avoid changing working directory):
```bash
go test -C connectrpc-axum-examples/go-client -v -timeout 300s
```

The Go tests:
1. Build all Rust example servers (once, cached)
2. Start each server, wait for it to be ready
3. Run Go client tests against each server
4. Validate responses match expected behavior

## Success Criteria

**Unit Tests**: All tests pass with exit code 0

**Rust Client Tests**: All 7 tests pass ("=== All tests passed! ===" in output)

**Go Integration Tests**: All tests pass (PASS in output)

## Integration Test Matrix

### Rust Client Tests

| Test | Server | Protocol | Test Type |
|------|--------|----------|-----------|
| unary-client | connect-unary | Connect | Unary (JSON + Proto) |

### Go Client Tests

| Test | Server | Protocol | Test Type |
|------|--------|----------|-----------|
| TestConnectUnary | connect-unary | Connect | Unary |
| TestConnectServerStream | connect-server-stream | Connect | Server streaming |
| TestTonicUnaryConnect | tonic-unary | Connect | Unary |
| TestTonicUnaryGRPC | tonic-unary | gRPC | Unary |
| TestTonicServerStreamConnect | tonic-server-stream | Connect | Server streaming |
| TestTonicServerStreamGRPC | tonic-server-stream | gRPC | Server streaming |
| TestTonicBidiStreamConnectUnary | tonic-bidi-stream | Connect | Unary |
| TestTonicBidiStreamGRPC | tonic-bidi-stream | gRPC | Bidi streaming |
| TestGRPCWeb | grpc-web | gRPC-Web | Unary |
| TestStreamingErrorHandling | streaming-error-repro | Connect | Stream error handling |
| TestProtocolVersion | protocol-version | Connect | Protocol header validation |
| TestTimeout | timeout | Connect | Connect-Timeout-Ms enforcement |
| TestExtractorConnectError | extractor-connect-error | Connect | Extractor rejection with ConnectError |
| TestExtractorHTTPResponse | extractor-http-response | Connect | Extractor rejection with plain HTTP |

## Failure Handling

**Unit test failures**: Check the specific test name and error message

**Integration test failures**:
1. Note which specific test failed from the output
2. Check if the server started (look for "Server ready" message)
3. Check the Go client error message for details
4. Common issues:
   - Port 3000 already in use
   - Missing Go dependencies (run `go mod tidy` in go-client/)
   - Build errors (run `cargo build --features tonic` first)

## Report Format

```
Unit Tests: [PASS/FAIL]
- Passed: X
- Failed: Y

Rust Client Tests: [PASS/FAIL]
- 7 tests passed

Go Integration Tests: [PASS/FAIL]
- X tests passed
```

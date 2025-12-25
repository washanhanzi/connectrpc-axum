# test

Run the complete test suite for connectrpc-axum: unit tests, doc tests, and integration tests with Go clients.

## Instructions

Run both test suites in order:

### 1. Unit Tests

```bash
cargo test
```

### 2. Integration Tests

```bash
./connectrpc-axum-examples/test-all.sh
```

This script:
1. Builds and starts each Rust example server
2. Runs a Go client against each server
3. Validates responses match expected behavior

## Success Criteria

**Unit Tests**: All tests pass with exit code 0

**Integration Tests**:
1. Exit code is 0
2. All 10 tests pass - summary shows "All 10 tests passed!"
3. No FAIL markers in results

## Integration Test Matrix

| # | Server | Protocol | Test Type |
|---|--------|----------|-----------|
| 1 | connect-unary | Connect | Unary |
| 2 | connect-server-stream | Connect | Server streaming |
| 3 | tonic-unary | Connect | Unary |
| 4 | tonic-unary | gRPC | Unary |
| 5 | tonic-server-stream | Connect | Server streaming |
| 6 | tonic-server-stream | gRPC | Server streaming |
| 7 | tonic-bidi-stream | Connect | Unary |
| 8 | tonic-bidi-stream | gRPC | Bidi streaming |
| 9 | grpc-web | gRPC-Web | Unary |
| 10 | streaming-error-repro | Connect | Stream error handling |

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

Integration Tests: [PASS/FAIL]
- Passed: X/10
- Failed: Y
```

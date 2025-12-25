# test-integration

Run the full integration test suite for connectrpc-axum examples. Use when verifying protocol implementations work correctly with real Go clients.

## Instructions

Execute the integration test script:

```bash
./connectrpc-axum-examples/test-all.sh
```

## Success Criteria

The tests pass when ALL of these conditions are met:

1. **Exit code is 0** - The script returns exit code 0
2. **All 10 tests pass** - The summary shows "All 10 tests passed!"
3. **No FAIL markers** - No test shows FAIL in the results

## Verification Chain

The Go client validates each response:

| Check | What It Validates |
|-------|-------------------|
| `err != nil` | Connection, RPC errors, missing trailers |
| `message == ""` | Server returning empty responses |
| `Contains(name)` | Server echoing back request data correctly |
| `msgCount == 0` | Streams yielding at least one message |
| `StatusCode != 200` | HTTP-level failures (gRPC-Web) |

If any validation fails, Go client calls `log.Fatalf()` which exits with code 1.

## Test Matrix

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

If any test fails:
1. Note which specific test failed from the output
2. Check if the server started (look for "Server ready" message)
3. Check the Go client error message for details
4. Common issues:
   - Port 3000 already in use
   - Missing Go dependencies (run `go mod tidy` in go-client/)
   - Build errors (run `cargo build --features tonic` first)

## Report Format

After running, report results as:

```
Integration Tests: [PASS/FAIL]
- Total: 10 tests
- Passed: X
- Failed: Y
- Exit code: Z
```

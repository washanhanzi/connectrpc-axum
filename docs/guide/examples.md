# Examples

See the [connectrpc-axum-examples](https://github.com/phlx-io/connectrpc-axum/tree/main/connectrpc-axum-examples) directory for complete working examples.

| Example | Description |
|---------|-------------|
| `connect-unary` | Pure Connect unary RPC |
| `connect-server-stream` | Pure Connect server streaming |
| `connect-client-stream` | Pure Connect client streaming |
| `connect-bidi-stream` | Pure Connect bidirectional streaming |
| `tonic-unary` | Connect + gRPC unary (dual protocol) |
| `tonic-server-stream` | Connect + gRPC streaming (dual protocol) |
| `tonic-bidi-stream` | Bidirectional streaming (gRPC only) |
| `grpc-web` | gRPC-Web browser support |
| `timeout` | Connect-Timeout-Ms header handling |
| `protocol-version` | Connect-Protocol-Version header validation |
| `streaming-error-repro` | Streaming error handling demonstration |

## Running Examples

```bash
cd connectrpc-axum-examples
cargo run --bin connect-unary
```

For examples with tonic features:

```bash
cargo run --bin tonic-unary --features tonic
cargo run --bin grpc-web --features tonic-web
```

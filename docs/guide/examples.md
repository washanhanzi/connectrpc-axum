# Examples

See the [connectrpc-axum-examples](https://github.com/washanhanzi/connectrpc-axum/tree/main/connectrpc-axum-examples) directory for complete working examples.

## Connect Protocol

| Example | Description |
|---------|-------------|
| `connect-unary` | Pure Connect unary RPC |
| `connect-server-stream` | Pure Connect server streaming |
| `connect-client-stream` | Pure Connect client streaming |
| `connect-bidi-stream` | Pure Connect bidirectional streaming |
| `get-request` | GET request support for idempotent unary RPCs |

## Tonic/gRPC Integration

| Example | Description |
|---------|-------------|
| `tonic-unary` | Connect + gRPC unary (dual protocol) |
| `tonic-server-stream` | Connect + gRPC streaming (dual protocol) |
| `tonic-bidi-stream` | Bidirectional streaming (gRPC only) |
| `tonic-extractor` | Multiple extractors with TonicCompatibleBuilder |
| `grpc-web` | gRPC-Web browser support |

## Error Handling

| Example | Description |
|---------|-------------|
| `error-details` | Returning google.rpc error details (e.g., RetryInfo) |
| `extractor-connect-error` | Extractor rejection with ConnectError |
| `extractor-http-response` | Extractor rejection with plain HTTP response |
| `unary-error-metadata` | Unary errors with custom metadata headers |
| `endstream-metadata` | EndStream frame metadata in streaming |
| `streaming-error-repro` | Streaming error handling demonstration |

## Protocol Features

| Example | Description |
|---------|-------------|
| `timeout` | Connect-Timeout-Ms header handling |
| `protocol-version` | Connect-Protocol-Version header validation |
| `streaming-compression` | Per-message compression in streaming responses |
| `client-streaming-compression` | Per-message decompression in streaming requests |

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

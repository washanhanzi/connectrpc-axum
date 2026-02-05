# Examples

See the [connectrpc-axum-examples](https://github.com/washanhanzi/connectrpc-axum/tree/main/connectrpc-axum-examples) directory for complete working examples.

## Connect Protocol

| Example | Description |
|---------|-------------|
| `connect-unary` | Pure Connect unary RPC |
| `connect-server-stream` | Pure Connect server streaming |
| `connect-client-stream` | Pure Connect client streaming |
| `connect-bidi-stream` | Pure Connect bidirectional streaming |

## Tonic/gRPC Integration

| Example | Description |
|---------|-------------|
| `tonic-unary` | Connect + gRPC unary (dual protocol) |
| `tonic-server-stream` | Connect + gRPC streaming (dual protocol) |
| `tonic-bidi-stream` | Bidirectional streaming (gRPC only) |
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

## Compression

| Example | Description |
|---------|-------------|
| `streaming-compression` | Per-message compression in streaming responses |
| `client-streaming-compression` | Per-message decompression in streaming requests |
| `streaming-compression-algos` | Streaming compression with all algorithms |
| `client-streaming-compression-algos` | Client streaming decompression with all algorithms |
| `unary-compression-algos` | Unary compression with all algorithms |

## Connect RPC Client

| Example | Description |
|---------|-------------|
| `client/unary-client` | Basic unary RPC client |
| `client/server-stream-client` | Server streaming client |
| `client/client-stream-client` | Client streaming client |
| `client/bidi-stream-client` | Bidirectional streaming client |
| `client/typed-client` | Type-safe generated client |
| `client/streaming-interceptor-client` | Streaming with interceptors |
| `client/message-interceptor-client` | Message-level interceptor |
| `client/typed-interceptor-client` | Typed interceptor on generated clients |

## Running Examples

```bash
cd connectrpc-axum-examples
cargo run --bin connect-unary
```

For client examples:

```bash
# Start a server first
cargo run --bin connect-unary &

# Run the client
cargo run --bin unary-client
```

For examples with tonic features:

```bash
cargo run --bin tonic-unary --features tonic
cargo run --bin grpc-web --features tonic-web
```

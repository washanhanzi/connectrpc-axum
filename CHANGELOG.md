# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

#### Client Library (`connectrpc-axum-client`)
- New Connect RPC client implementation
- Unary RPC calls with JSON and protobuf encoding
- Server streaming RPC support with `StreamBody` wrapper
- Client streaming RPC support with `FrameEncoder`
- Bidirectional streaming RPC support (requires HTTP/2)
- Request compression (gzip, brotli, zstd) with configurable thresholds
- Response decompression
- Middleware support via `reqwest-middleware`
- `ConnectResponse<T>` wrapper with metadata access
- `Metadata` type for accessing response headers and trailers

#### Code Generation (`connectrpc-axum-build`)
- Generated typed client structs with `.with_connect_client()` option
- Service name and procedure path constants
- Typed methods for all RPC patterns:
  - Unary: `async fn method(&self, request) -> Result<ConnectResponse<T>>`
  - Server streaming: returns `StreamBody<FrameDecoder<...>>`
  - Client streaming: takes `impl Stream<Item = T>`
  - Bidirectional streaming: takes stream, returns stream
- `ClientBuilder` pattern for configuring encoding and compression

#### Core Library (`connectrpc-axum-core`)
- Extracted shared protocol code from `connectrpc-axum`
- `Codec` trait and implementations (gzip, deflate, brotli, zstd)
- `CompressionConfig` and `CompressionEncoding` types
- Envelope frame parsing and encoding functions
- `ConnectError` and `Code` types
- `Metadata` type for header management

### Changed
- `connectrpc-axum` now depends on `connectrpc-axum-core` for shared types

## [0.1.0-alpha.1] - Initial Release

- Initial alpha release with Connect RPC server support
- Axum handler integration
- Optional Tonic integration for gRPC/gRPC-Web
- JSON and protobuf encoding
- Server-side streaming support

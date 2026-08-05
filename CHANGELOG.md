# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.3] - 2026-08-05

### Fixed

- Oversized HTTP/1 request bodies are drained without buffering before returning
  `resource_exhausted`, allowing clients to receive the RPC error instead of a broken pipe.

## [0.2.2] - 2026-08-05

### Added

- `Status::metadata()` and `ClientError::metadata()` expose received RPC response metadata.
- Raw and generated client builders support service-wide default request headers. Per-call
  headers override defaults, and interceptors may replace the merged application headers.

### Fixed

- Unary `Trailer-*` response headers are normalized to unprefixed metadata names.
- Streaming EndStream errors carry the union of initial response headers and EndStream
  metadata. Successful client-streaming responses also retain EndStream metadata.
- CI tests the client without default features to guard existing plain HTTP support against
  regression.

## [0.1.0-alpha.1] - Initial Release

- Initial alpha release with Connect RPC server support
- Axum handler integration
- Optional Tonic integration for gRPC/gRPC-Web
- JSON and protobuf encoding
- Server-side streaming support

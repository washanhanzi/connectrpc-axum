# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

> Think carefully and implement the most concise solution that changes as little code as possible.

## USE SUB-AGENTS FOR CONTEXT OPTIMIZATION

### 1. Always use the file-analyzer sub-agent when asked to read files.
The file-analyzer agent is an expert in extracting and summarizing critical information from files, particularly log files and verbose outputs. It provides concise, actionable summaries that preserve essential information while dramatically reducing context usage.

### 2. Always use the code-analyzer sub-agent when asked to search code, analyze code, research bugs, or trace logic flow.

The code-analyzer agent is an expert in code analysis, logic tracing, and vulnerability detection. It provides concise, actionable summaries that preserve essential information while dramatically reducing context usage.

### 3. Always use the test-runner sub-agent to run tests and analyze the test results.

Using the test-runner agent ensures:

- Full test output is captured for debugging
- Main conversation stays clean and focused
- Context usage is optimized
- All issues are properly surfaced
- No approval dialogs interrupt the workflow

## Philosophy

### Error Handling

- **Fail fast** for critical configuration (missing text model)
- **Log and continue** for optional features (extraction model)
- **Graceful degradation** when external services unavailable
- **User-friendly messages** through resilience layer

### Testing

- Always use the test-runner agent to execute tests.
- Do not use mock services for anything ever.
- Do not move on to the next test until the current test is complete.
- If the test fails, consider checking if the test is structured correctly before deciding we need to refactor the codebase.
- Tests to be verbose so we can use them for debugging.


## Tone and Behavior

- Criticism is welcome. Please tell me when I am wrong or mistaken, or even when you think I might be wrong or mistaken.
- Please tell me if there is a better approach than the one I am taking.
- Please tell me if there is a relevant standard or convention that I appear to be unaware of.
- Be skeptical.
- Be concise.
- Short summaries are OK, but don't give an extended breakdown unless we are working through the details of a plan.
- Do not flatter, and do not give compliments unless I am specifically asking for your judgement.
- Occasional pleasantries are fine.
- Feel free to ask many questions. If you are in doubt of my intent, don't guess. Ask.

## ABSOLUTE RULES:

- NO PARTIAL IMPLEMENTATION
- NO SIMPLIFICATION : no "//This is simplified stuff for now, complete implementation would blablabla"
- NO CODE DUPLICATION : check existing codebase to reuse functions and constants Read files before writing new functions. Use common sense function name to find them easily.
- NO DEAD CODE : either use or delete from codebase completely
- IMPLEMENT TEST FOR EVERY FUNCTIONS
- NO CHEATER TESTS : test must be accurate, reflect real usage and be designed to reveal flaws. No useless tests! Design tests to be verbose so we can use them for debuging.
- NO INCONSISTENT NAMING - read existing codebase naming patterns.
- NO OVER-ENGINEERING - Don't add unnecessary abstractions, factory patterns, or middleware when simple functions would work. Don't think "enterprise" when you need "working"
- NO MIXED CONCERNS - Don't put validation logic inside API handlers, database queries inside UI components, etc. instead of proper separation
- NO RESOURCE LEAKS - Don't forget to close database connections, clear timeouts, remove event listeners, or clean up file handles

## Project Overview

ConnectRPC-Axum is a Rust library that brings the Connect RPC framework to Axum web servers. It consists of three main crates:
- `connectrpc-axum`: Runtime library providing Connect RPC support for Axum
- `connectrpc-axum-build`: Code generation tool for creating routes from `.proto` files
- `connectrpc-axum-examples`: Example application demonstrating usage

## Development Commands

### Building the Project
```bash
# Build all workspace members
cargo build

# Build a specific crate
cargo build -p connectrpc-axum
cargo build -p connectrpc-axum-build
cargo build -p connectrpc-axum-examples

# Build with release optimizations
cargo build --release
```

### Running the Example
```bash
cargo run -p connectrpc-axum-examples
```
The example server runs on `http://127.0.0.1:3030`

### Testing Endpoints
```bash
# Test unary RPC
curl -X POST http://localhost:3030/hello.HelloWorldService/SayHello \
     -H "Content-Type: application/json" \
     -d '{"name":"Axum"}'

# Test streaming RPC  
curl -X POST http://localhost:3030/hello.HelloWorldService/SayHelloStream \
     -H "Content-Type: application/json" \
     -d '{"name":"Stream"}'
```

### Common Cargo Commands
```bash
# Check for compilation errors without building
cargo check

# Format code
cargo fmt

# Run clippy linter
cargo clippy

# Clean build artifacts
cargo clean

# Update dependencies
cargo update
```

## Architecture

### Code Generation Flow
1. Proto files (`.proto`) define service interfaces
2. `build.rs` uses `connectrpc-axum-build` to generate Rust code at compile time
3. Generated code includes a `routes()` function that creates Axum routes with a `Handlers` struct
4. Service implementations are standard Axum handlers using `ConnectRequest` and `ConnectResponse`

### Key Types and Traits

#### Core Types
- `ConnectRequest<T>`: Extractor for request payloads (MUST be the last parameter in handlers)
- `ConnectResponse<T>`: Response wrapper for unary RPCs
- `ConnectStreamResponse<S>`: Response wrapper for streaming RPCs
- `ConnectError`: Error type mapping to Connect protocol errors with proper HTTP status codes

#### Handler System
The library provides a specialized handler system (`connectrpc-axum/src/handler.rs`):
- `ConnectHandler` trait: Core trait for Connect RPC handlers
- `ConnectService`: Tower service wrapper that bridges handlers to Axum routing
- Helper functions for different handler patterns:
  - `simple_connect_handler`: For handlers that only take `ConnectRequest<T>`
  - `stateful_connect_handler`: For handlers with `State<S>` and `ConnectRequest<T>`
  - `extractor_connect_handler`: For handlers with additional Axum extractors

**IMPORTANT**: `ConnectRequest` must always be the last parameter in handler functions because it consumes the request body.

### Handler Pattern Examples
```rust
// Unary handler with state
async fn handler(
    State(state): State<AppState>,
    Query(params): Query<Params>,        // Other extractors come first
    ConnectRequest(req): ConnectRequest<Req>  // ConnectRequest MUST be last
) -> Result<ConnectResponse<Res>, ConnectError>

// Streaming handler
async fn stream_handler(
    ConnectRequest(req): ConnectRequest<Req>
) -> ConnectStreamResponse<impl Stream<Item = Result<Res, ConnectError>>>
```

### Generated Code Structure
The build process generates a module for each service with:
- A `Handlers` struct containing fields for each RPC method
- A `routes()` function that accepts handler instances and state
- Proper routing paths following Connect protocol: `/{package}.{service}/{method}`

### Dependencies
The project uses `connect-core` from the Connect Rust repository for core protocol support. JSON serialization requires adding serde attributes via prost_build configuration in `build.rs`.
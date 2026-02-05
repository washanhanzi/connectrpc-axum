# Refactor `connectrpc-axum-examples` into `connectrpc-axum-test`

## Context

The `connectrpc-axum-examples` crate is actually a test/validation suite, not user-facing examples. Two interceptor client tests (`streaming-interceptor-client`, `typed-interceptor-client`) embed their own Rust servers, so they can't run against the Go reference server. This breaks the full compatibility matrix. The goal is to:

1. Rename the crate to `connectrpc-axum-test`
2. Reorganize binaries into `server/`, `client/`, `test/` subdirectories
3. Extract embedded servers into standalone binaries
4. Create a Go interceptor server that echoes headers
5. Achieve the full test matrix: every Rust client runs against both Rust and Go servers

## New Directory Layout

```
connectrpc-axum-test/
  Cargo.toml                    # renamed package
  build.rs                      # unchanged
  buf.yaml / buf.gen.yaml       # updated module paths
  Makefile.toml                 # updated crate references
  proto/                        # unchanged
  src/
    lib.rs                      # unchanged (just crate rename)
    bin/
      integration-test.rs       # updated: full matrix, GoServerType enum
      server/                   # ALL server binaries (moved from src/bin/)
        connect-unary.rs
        connect-server-stream.rs
        connect-client-stream.rs
        connect-bidi-stream.rs
        tonic-unary.rs
        ... (all existing servers)
        interceptor-echo.rs           # NEW: extracted from streaming-interceptor-client
        interceptor-typed-echo.rs     # NEW: extracted from typed-interceptor-client
      client/                   # ALL client binaries (already in src/bin/client/)
        unary-client.rs
        typed-client.rs
        server-stream-client.rs
        client-stream-client.rs
        bidi-stream-client.rs
        streaming-interceptor-client.rs   # refactored: removed embedded server
        typed-interceptor-client.rs       # refactored: removed embedded server
        message-interceptor-client.rs
      test/                     # Self-contained tests (embed own servers)
        interceptor-test.rs           # moved from src/bin/
        rpc-interceptor-test.rs       # moved from src/bin/
  go-client/                    # updated go.mod module paths
  go-server/                    # unchanged (reference server)
  go-server-interceptor/        # NEW: Go server that echoes headers
    main.go
    go.mod
    go.sum
```

## Implementation Phases

### Phase 1: Rename crate (mechanical)

1. `git mv connectrpc-axum-examples connectrpc-axum-test`
2. Update workspace `Cargo.toml`: change member `"connectrpc-axum-examples"` to `"connectrpc-axum-test"`
3. Update `connectrpc-axum-test/Cargo.toml`: `name = "connectrpc-axum-test"`
4. Global replace in all `.rs` files: `connectrpc_axum_examples` -> `connectrpc_axum_test`
5. Global replace in config files: `connectrpc-axum-examples` -> `connectrpc-axum-test` in:
   - `Makefile.toml` (root)
   - `connectrpc-axum-test/Makefile.toml`
   - `connectrpc-axum-test/Cargo.toml` (build-dependencies path)
   - Go files: `common_test.go` (cargo build command references)
6. Update Go module paths in all `go.mod` files and Go source imports:
   - `go-client/go.mod`: `github.com/connectrpc-axum/test/go-client`
   - `go-client/gen/go.mod`, `go-client/gen/genconnect/go.mod`: update accordingly
   - `go-server/go.mod`: `github.com/connectrpc-axum/test/go-server`
   - `go-server/main.go`: update import paths
   - All `*_test.go` files: update import paths
   - `buf.gen.yaml`: update `go_package_prefix`
7. Regenerate Go code: `buf generate` from `connectrpc-axum-test/`
8. Update references in documentation:
   - `README.md`, `docs/guide/examples.md`
   - `.claude/skills/` SKILL.md files
   - `TASKS.md`, `go-client/README.md`

**Verify:** `cargo build -p connectrpc-axum-test --features tonic` and `cargo test --workspace`

### Phase 2: Reorganize directory structure

1. Create directories: `src/bin/server/`, `src/bin/test/`
2. Move all server binaries from `src/bin/*.rs` to `src/bin/server/*.rs` (31 files):
   - `connect-unary.rs`, `connect-server-stream.rs`, `connect-bidi-stream.rs`, `connect-client-stream.rs`
   - `tonic-unary.rs`, `tonic-server-stream.rs`, `tonic-bidi-stream.rs`, `grpc-web.rs`
   - `protocol-version.rs`, `timeout.rs`, `get-request.rs`, `idempotency-get.rs`
   - `streaming-error-repro.rs`, `streaming-extractor.rs`, `tonic-extractor.rs`
   - `extractor-connect-error.rs`, `extractor-http-response.rs`
   - `endstream-metadata.rs`, `unary-error-metadata.rs`, `error-details.rs`
   - `axum-router.rs`
   - `streaming-compression.rs`, `client-streaming-compression.rs`
   - `streaming-compression-algos.rs`, `client-streaming-compression-algos.rs`, `unary-compression-algos.rs`
   - `send-max-bytes.rs`, `receive-max-bytes.rs`, `receive-max-bytes-5mb.rs`, `receive-max-bytes-unlimited.rs`
3. Move self-contained test binaries to `src/bin/test/`:
   - `interceptor-test.rs`, `rpc-interceptor-test.rs`
4. Update ALL `[[bin]]` path entries in `Cargo.toml` to reflect new locations
5. `integration-test.rs` stays at `src/bin/integration-test.rs`

**Verify:** `cargo build -p connectrpc-axum-test --features tonic`

### Phase 3: Extract embedded servers into standalone binaries

#### 3a. `src/bin/server/interceptor-echo.rs`
Extract from `streaming-interceptor-client.rs` (lines 47-213): the `say_hello_stream`, `echo_client_stream`, `echo_bidi_stream` handlers and `run_server` function. These handlers echo `x-interceptor-header` in response messages.

Server structure:
- Uses `connectrpc_axum_test::server_addr()` for port binding
- Registers HelloWorldService (say_hello_stream) and EchoService (echo_client_stream, echo_bidi_stream)
- Uses `MakeServiceBuilder` for HTTP/2 h2c support

#### 3b. `src/bin/server/interceptor-typed-echo.rs`
Extract from `typed-interceptor-client.rs` (lines 47-184): the `say_hello`, `say_hello_stream`, `echo`, `echo_client_stream`, `echo_bidi_stream` handlers. These handlers echo `x-custom-header` in response messages.

Server structure:
- Uses `connectrpc_axum_test::server_addr()` for port binding
- Registers both services with all 5 handlers
- Uses `MakeServiceBuilder`

Add `[[bin]]` entries for both new servers.

**Verify:** `cargo build --bin interceptor-echo --bin interceptor-typed-echo --no-default-features`

### Phase 4: Refactor interceptor clients to use SERVER_URL

#### 4a. `streaming-interceptor-client.rs`
- Remove all server code (handlers, `run_server`, imports for server types, `PORT_COUNTER`)
- Add SERVER_URL support: `env::args().nth(1).or_else(|| env::var("SERVER_URL").ok()).unwrap_or_else(|| "http://localhost:3000".to_string())`
- Remove `tokio::spawn(run_server(addr))` and the 200ms sleep
- All 9 test assertions remain unchanged

#### 4b. `typed-interceptor-client.rs`
- Remove server code (handlers, `run_server`, imports)
- Add SERVER_URL support (same pattern)
- `CountingInterceptor` struct stays (it's a client-side interceptor)
- All 7 tests remain unchanged

**Verify:**
```
# Terminal 1: cargo run --bin interceptor-echo --no-default-features
# Terminal 2: SERVER_URL=http://localhost:3000 cargo run --bin streaming-interceptor-client --no-default-features
# Terminal 1: cargo run --bin interceptor-typed-echo --no-default-features
# Terminal 2: SERVER_URL=http://localhost:3000 cargo run --bin typed-interceptor-client --no-default-features
```

### Phase 5: Create Go interceptor server

Create `go-server-interceptor/` with a Go server that echoes custom headers in responses.

**`go-server-interceptor/go.mod`:**
- Module: `github.com/connectrpc-axum/test/go-server-interceptor`
- Same dependencies as `go-server/go.mod`
- Replace directives pointing to `../go-client/gen`

**`go-server-interceptor/main.go`:**
Implements all 6 RPC methods with header echoing:

| Method | Header Read | Echo Format |
|--------|------------|-------------|
| SayHello | `x-custom-header` | `"Hello, {name}! Header: {value}"` |
| SayHelloStream | `x-interceptor-header` or `x-custom-header` | First message: `"Stream 1 for {name}. Header: {value}"` |
| GetGreeting | `x-custom-header` | `"Greetings #{count}, {name}! Header: {value}"` |
| Echo | `x-custom-header` | `"{message}\|header:{value}"` |
| EchoClientStream | `x-interceptor-header` | `"Received {n} messages [{list}]. Interceptor: {value}"` |
| EchoBidiStream | `x-interceptor-header` | First response: `"Bidi #0: {msg} [Interceptor: {value}]"` |

Response formats match exactly what the Rust interceptor clients assert. Supports `PORT` env var, `/__server_id` returns `"go-server-interceptor"`.

**Verify:**
```
# Terminal 1: cd go-server-interceptor && go run .
# Terminal 2: SERVER_URL=http://localhost:3000 cargo run --bin streaming-interceptor-client --no-default-features
# Terminal 2: SERVER_URL=http://localhost:3000 cargo run --bin typed-interceptor-client --no-default-features
```

### Phase 6: Update integration test runner

**File:** `src/bin/integration-test.rs`

1. Add `GoServerType` enum:
   ```rust
   enum GoServerType {
       Reference,    // go-server/
       Interceptor,  // go-server-interceptor/
   }
   ```

2. Update `CrossImplTest` struct:
   ```rust
   struct CrossImplTest {
       name: &'static str,
       client_bin: &'static str,
       go_server: GoServerType,
   }
   ```

3. Add `start_go_interceptor_server()` function (same as `start_go_server()` but points to `go-server-interceptor/`)

4. Add to `get_rust_client_tests()`:
   ```rust
   RustClientTest { name: "Rust Client: Streaming Interceptor", server: ServerConfig { name: "interceptor-echo", features: None }, client_bin: "streaming-interceptor-client" },
   RustClientTest { name: "Rust Client: Typed Interceptor", server: ServerConfig { name: "interceptor-typed-echo", features: None }, client_bin: "typed-interceptor-client" },
   ```

5. Add to `get_cross_impl_tests()`:
   ```rust
   CrossImplTest { name: "Cross-Impl: Streaming Interceptor → Go Interceptor Server", client_bin: "streaming-interceptor-client", go_server: GoServerType::Interceptor },
   CrossImplTest { name: "Cross-Impl: Typed Interceptor → Go Interceptor Server", client_bin: "typed-interceptor-client", go_server: GoServerType::Interceptor },
   ```

6. Update existing cross-impl entries to include `go_server: GoServerType::Reference`

7. Update `run_cross_impl_test()` to select server based on `GoServerType`

8. Update all `"-p", "connectrpc-axum-examples"` to `"-p", "connectrpc-axum-test"`

9. Update `start_go_server()` path: `root_dir.join("connectrpc-axum-test/go-server")`

**Verify:** `cargo run --bin integration-test -- --rust-client` and `cargo run --bin integration-test -- --cross-impl`

### Phase 7: Update Makefile.toml and docs

1. Root `Makefile.toml`: replace all `connectrpc-axum-examples` with `connectrpc-axum-test`
2. `connectrpc-axum-test/Makefile.toml`: update Go server build tasks to include `go-server-interceptor`
3. Update `.claude/skills/` SKILL.md files (test, create-integration-test, resolve-issue)
4. Update `README.md`, `docs/guide/examples.md`
5. Update `TASKS.md`, `go-client/README.md`

**Verify:** `cargo make test`

## Final Test Matrix

| Rust Client | Rust Server | Go Server |
|-------------|-------------|-----------|
| unary-client | connect-unary | go-server (reference) |
| server-stream-client | connect-server-stream | go-server (reference) |
| client-stream-client | connect-client-stream | go-server (reference) |
| bidi-stream-client | connect-bidi-stream | go-server (reference) |
| typed-client | connect-unary | go-server (reference) |
| message-interceptor-client | *(uses go-server directly)* | go-server (reference) |
| streaming-interceptor-client | **interceptor-echo** | **go-server-interceptor** |
| typed-interceptor-client | **interceptor-typed-echo** | **go-server-interceptor** |

| Go Client | Rust Server |
|-----------|-------------|
| 23 existing test configs | All existing servers (unchanged) |

## Files to Modify

| File | Change |
|------|--------|
| `Cargo.toml` (workspace) | Rename member |
| `connectrpc-axum-test/Cargo.toml` | Rename package, update all `[[bin]]` paths, add 2 new bins |
| All `*.rs` in the crate (~45 files) | `connectrpc_axum_examples` -> `connectrpc_axum_test` |
| `src/bin/integration-test.rs` | Add GoServerType, new test entries, update package refs |
| `src/bin/client/streaming-interceptor-client.rs` | Remove embedded server, add SERVER_URL |
| `src/bin/client/typed-interceptor-client.rs` | Remove embedded server, add SERVER_URL |
| `src/bin/server/interceptor-echo.rs` | **NEW** - extracted server |
| `src/bin/server/interceptor-typed-echo.rs` | **NEW** - extracted server |
| `go-server-interceptor/main.go` | **NEW** - Go interceptor server |
| `go-server-interceptor/go.mod` | **NEW** |
| `Makefile.toml` (root) | Update crate references |
| `connectrpc-axum-test/Makefile.toml` | Update crate references, add go-server-interceptor |
| `go-client/go.mod` + all `go.mod` files | Update module paths |
| `go-server/main.go` | Update import paths |
| All `go-client/*_test.go` | Update import paths |
| `buf.gen.yaml` | Update go_package_prefix |
| `.claude/skills/` SKILL.md files | Update crate name references |
| `README.md`, `docs/guide/examples.md` | Update crate name references |

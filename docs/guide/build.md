# Build Configuration

This guide is organized by `CompileBuilder` methods so each option is introduced once.

## Builder Flow

Typical method order:

1. Pick source (`compile_dir` or `compile_protos`)
2. Pick generation mode (`no_connect_server`, `with_connect_client`, `with_tonic`, `with_tonic_client`)
3. Add config hooks (`with_buffa_config`, `extern_path`, tonic config hooks)
4. Choose output/module options (`out_dir`, `include_file`, `extern_module`)
5. Run `compile()`

## Source Methods

### `compile_dir("proto")`

Auto-discovers `.proto` files recursively from a directory.

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto").compile()?;
    Ok(())
}
```

### `compile_protos(&[...], &[...])`

Use explicit proto files and include directories.

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_protos(
        &["proto/service.proto", "proto/messages.proto"],
        &["proto", "third_party"],
    )
    .compile()?;
    Ok(())
}
```

### `builder()`

Use `builder()` when you want to set options before selecting a source:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::builder()
        .include_file("protos.rs")
        .compile_dir("proto")
        .compile()?;
    Ok(())
}
```

### Multiple Sources

Use separate builders (one source per builder). If multiple builders write to the same output directory, use different include file names.

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto/api")
        .include_file("protos_api.rs")
        .compile()?;

    connectrpc_axum_build::compile_dir("proto/internal")
        .include_file("protos_internal.rs")
        .compile()?;
    Ok(())
}
```

## Generation Mode Methods

### `no_connect_server()`

Disables Connect server builder generation. Message/types code is still generated.

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .no_connect_server()
        .compile()?;
    Ok(())
}
```

### `with_connect_client()`

Generates typed Connect clients.

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_connect_client()
        .compile()?;
    Ok(())
}
```

### `with_tonic()` and `with_tonic_prost_config(...)`

Requires `tonic` feature:

```toml
[build-dependencies]
connectrpc-axum-build = { version = "*", features = ["tonic"] }
```

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_tonic()
        .with_tonic_prost_config(|builder| builder.build_transport(false))
        .compile()?;
    Ok(())
}
```

`no_connect_server()` and `with_tonic()` cannot be combined.

### `with_tonic_request_mode(...)`

Switch tonic server request types between owned messages and zero-copy `View<T>` wrappers.

Requires `tonic` feature:

```rust
use connectrpc_axum_build::TonicRequestMode;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_tonic()
        .with_tonic_request_mode(TonicRequestMode::View)
        .compile()?;
    Ok(())
}
```

This only changes generated tonic server handler signatures. Connect handlers can opt into the same request mode directly with `ViewRequest<T>`.

### `with_tonic_client()` and `with_tonic_client_config(...)`

Requires `tonic-client` feature:

```toml
[build-dependencies]
connectrpc-axum-build = { version = "*", features = ["tonic-client"] }
```

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_tonic_client()
        .with_tonic_client_config(|builder| builder.build_transport(false))
        .compile()?;
    Ok(())
}
```

## Configuration Hooks

### `with_buffa_config(...)`

Customize the underlying `buffa_build::Config`.

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_buffa_config(|config| {
            config
                .generate_arbitrary(true)
                .strict_utf8_mapping(true)
        })
        .compile()?;
    Ok(())
}
```

`connectrpc-axum-build` always drives Buffa from a descriptor set, so output directory, requested files, and shared extern-path wiring are applied after this hook.

### `extern_path(...)`

Declare shared protobuf-to-Rust type mappings once and reuse them across:

- Buffa message generation
- Connect sidecar generation
- tonic server/client sidecars

If you import Google well-known types and do not generate them locally, `.google.protobuf` defaults to `::buffa_types::google::protobuf`.

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .extern_path(".acme.common.v1", "::shared_protos::acme::common::v1")
        .compile()?;
    Ok(())
}
```

### `fetch_protoc(...)`

Automatically downloads and configures `protoc`.

Requires `fetch-protoc` feature:

```toml
[build-dependencies]
connectrpc-axum-build = { version = "*", features = ["fetch-protoc"] }
```

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .fetch_protoc(None, None)?
        .compile()?;
    Ok(())
}
```

You can also specify version/path:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .fetch_protoc(Some("30.0"), Some(Path::new("/tmp/protoc")))?
        .compile()?;
    Ok(())
}
```

## Output and Module Methods

### `out_dir("...")`

Writes generated files to a custom directory instead of `OUT_DIR`.

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .out_dir("src/generated")
        .compile()?;
    Ok(())
}
```

### `include_file("protos.rs")`

Generates a single module tree include file.

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .include_file("protos.rs")
        .compile()?;
    Ok(())
}
```

Use in crate code:

```rust
// Default OUT_DIR mode
include!(concat!(env!("OUT_DIR"), "/protos.rs"));
```

```rust
// Custom out_dir("src/generated") mode
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/generated/protos.rs"));
```

When `out_dir(...)` is set, nested includes inside generated `protos.rs` are written as absolute paths.

### `extern_module("google.protobuf", "::pbjson_types")`

Adds a re-export shim in generated include file for externalized proto modules.

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .extern_path(".acme.common.v1", "::shared_protos::acme::common::v1")
        .include_file("protos.rs")
        .extern_module("acme.common.v1", "::shared_protos::acme::common::v1")
        .compile()?;
    Ok(())
}
```

## What Gets Generated

Depending on enabled methods/features:

- Buffa message types with generated borrowed view types and serde support
- Generated `HasView` glue so handlers can use `View<T>`
- Connect service builders (unless `no_connect_server()` is used)
- Connect route paths
- Typed Connect clients (if `with_connect_client()`)
- Tonic server stubs (if `with_tonic()`)
- Tonic server request signatures using owned messages or `View<T>` (via `with_tonic_request_mode(...)`)
- Tonic client stubs (if `with_tonic_client()`)

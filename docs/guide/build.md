# Build Configuration

This guide is organized by `CompileBuilder` methods so each option is introduced once.

## Builder Flow

Typical method order:

1. Pick source (`compile_dir` or `compile_protos`)
2. Pick generation mode (`no_connect_server`, `with_connect_client`, `with_tonic`, `with_tonic_client`)
3. Add config hooks (`with_prost_config`, `with_pbjson_config`, tonic config hooks)
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

### `with_prost_config(...)`

Customize `prost_build::Config` (type/field attributes, extern paths, etc.).

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_prost_config(|config| {
            config.type_attribute(".", "#[derive(Hash)]");
            config.field_attribute("MyMessage.my_field", "#[serde(skip)]");
        })
        .compile()?;
    Ok(())
}
```

### `with_pbjson_config(...)`

Customize `pbjson_build::Builder`.

When you map protobuf packages with `prost` extern paths, configure matching pbjson extern paths too:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_prost_config(|config| {
            config
                .compile_well_known_types()
                .extern_path(".google.protobuf", "::pbjson_types");
        })
        .with_pbjson_config(|builder| {
            builder.extern_path(".google.protobuf", "::pbjson_types");
        })
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
        .with_prost_config(|config| {
            config.compile_well_known_types();
            config.extern_path(".google.protobuf", "::pbjson_types");
        })
        .include_file("protos.rs")
        .extern_module("google.protobuf", "::pbjson_types")
        .compile()?;
    Ok(())
}
```

## What Gets Generated

Depending on enabled methods/features:

- Message types with `prost::Message` + `serde` derives
- Connect service builders (unless `no_connect_server()` is used)
- Connect route paths
- Typed Connect clients (if `with_connect_client()`)
- Tonic server stubs (if `with_tonic()`)
- Tonic client stubs (if `with_tonic_client()`)

# Build Configuration

Configure code generation in your `build.rs` file.

## Basic Setup

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto").compile()?;
    Ok(())
}
```

## Automatic protoc Download

Use `.fetch_protoc()` to automatically download the protoc compiler. This is useful when you don't want to require protoc to be installed on the build machine.

::: warning Feature Required
The `fetch_protoc()` method requires the `fetch-protoc` feature flag:

```toml
[build-dependencies]
connectrpc-axum-build = { version = "*", features = ["fetch-protoc"] }
```
:::

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .fetch_protoc(None, None)?  // Downloads protoc 31.1 to OUT_DIR
        .compile()?;
    Ok(())
}
```

You can specify a custom version or download path:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .fetch_protoc(Some("30.0"), Some(Path::new("/tmp/protoc")))?
        .compile()?;
    Ok(())
}
```

The downloaded binary is cached, so subsequent builds reuse it. The `PROTOC` environment variable is set automatically so prost-build uses the downloaded binary.

## Custom Output Directory

By default, generated code is written to `OUT_DIR` (set by Cargo during build). Use `.out_dir()` to specify a custom output directory:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .out_dir("src/generated")
        .compile()?;
    Ok(())
}
```

This is useful when you want to commit generated code to version control or inspect it more easily.

## Prost Configuration

Use `.with_prost_config()` to customize `prost_build::Config`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_prost_config(|config| {
            // Add custom derives to all generated types
            config.type_attribute(".", "#[derive(Hash)]");

            // Add field attributes
            config.field_attribute("MyMessage.my_field", "#[serde(skip)]");
        })
        .compile()?;
    Ok(())
}
```

## Well-Known Types

To use Google's well-known types (`Timestamp`, `Duration`, `Any`, etc.), configure prost to compile them and map to `pbjson_types`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_prost_config(|config| {
            config
                .compile_well_known_types()
                .extern_path(".google.protobuf", "::pbjson_types");
        })
        .compile()?;
    Ok(())
}
```

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
pbjson-types = "0.8"
```

## Client Only (No Server)

Use `.no_connect_server()` to skip generating Connect server code. This is useful when building a client-only application:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .no_connect_server()
        .with_connect_client()  // Generate typed Connect client
        .compile()?;
    Ok(())
}
```

This generates:
- Message types with serde support
- Typed Connect client (if `.with_connect_client()` is called)
- Tonic client (if `.with_tonic_client()` is called)

## Tonic Configuration

Enable gRPC server support with `.with_tonic()`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_tonic()
        .compile()?;
    Ok(())
}
```

Requires the `tonic` feature in `Cargo.toml`:

```toml
[build-dependencies]
connectrpc-axum-build = { version = "*", features = ["tonic"] }
```

Customize tonic server generation with `.with_tonic_prost_config()` (available after `.with_tonic()`):

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_tonic()
        .with_tonic_prost_config(|builder| {
            builder.build_transport(false)
        })
        .compile()?;
    Ok(())
}
```

Enable gRPC client support with `.with_tonic_client()`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_tonic_client()
        .compile()?;
    Ok(())
}
```

Requires the `tonic-client` feature in `Cargo.toml`:

```toml
[build-dependencies]
connectrpc-axum-build = { version = "*", features = ["tonic-client"] }
```

Customize tonic client generation with `.with_tonic_client_config()` (available after `.with_tonic_client()`):

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_tonic_client()
        .with_tonic_client_config(|builder| {
            builder.build_transport(false)
        })
        .compile()?;
    Ok(())
}
```

**Note:** `.no_connect_server()` and `.with_tonic()` cannot be combined - the compiler enforces valid method chains at compile time.

## Generated Code

The compiler generates:

- **Message types** - Rust structs with `prost::Message` and `serde` derives
- **Service builders** - `{Service}ServiceBuilder` for registering handlers
- **Route paths** - `/<package>.<Service>/<Method>`

Include the generated code in your project:

```rust
mod pb {
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
}
pub use pb::*;
```

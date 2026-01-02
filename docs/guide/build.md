# Build Configuration

Configure code generation in your `build.rs` file.

## Basic Setup

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto").compile()?;
    Ok(())
}
```

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

To use Google's well-known types (`Timestamp`, `Duration`, `Any`, etc.), configure extern paths:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_prost_config(|config| {
            // Use pbjson_types for well-known types (recommended for JSON support)
            config.extern_path(".google.protobuf", "::pbjson_types");

            // OR use prost_types if you don't need JSON serialization
            // config.extern_path(".google.protobuf", "::prost_types");
        })
        .compile()?;
    Ok(())
}
```

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
pbjson-types = "0.8"  # For JSON-compatible well-known types
# OR
prost-types = "0.14"  # For binary-only well-known types
```

## Tonic Configuration

Enable gRPC support with `.with_tonic()`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_tonic()
        .compile()?;
    Ok(())
}
```

**Note:** Use `with_prost_config()` for all type customization (attributes, extern paths). The `with_tonic_prost_config()` method only affects service trait generation, not message types. See [Architecture - Code Generation](/guide/architecture#code-generation) for details.

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

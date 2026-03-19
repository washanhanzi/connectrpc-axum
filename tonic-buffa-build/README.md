# tonic-buffa-build

`tonic-buffa-build` generates tonic client and server stubs from protobuf
descriptors while reusing Buffa message types instead of generating `prost`
message structs.

Use this crate when your message types already exist and you want tonic code
that references them through `extern_path(...)`.

This crate only generates tonic service and client code. It does not generate
message structs. In a typical setup, message types come from `buffa-build` and
the generated tonic stubs point at those Rust paths.

## Capabilities

- Generate tonic clients, servers, or both
- Compile from `.proto` files or an existing `FileDescriptorSet`
- Reuse existing Rust message types with `extern_path(...)`
- Use a custom codec path for Buffa-backed messages
- Default `.google.protobuf` imports to `::buffa_types::google::protobuf`
- Emit a package-shaped include file such as `mod.rs`
- Optionally wrap server request messages with a view type

## Example `build.rs`

```rust
fn main() -> std::io::Result<()> {
    tonic_buffa_build::configure()
        .build_transport(false)
        .codec_path("crate::codec::BuffaCodec")
        .extern_path(".hello.v1", "crate::generated::hello::v1")
        .include_file("mod.rs")
        .compile_protos(&["proto/hello/v1/hello.proto"], &["proto"])
}
```

Then include the generated output from your crate:

```rust
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/mod.rs"));
}
```

In this setup:

- Buffa message types live at `crate::generated::hello::v1`
- tonic client and server code is generated into `OUT_DIR`
- generated methods use `crate::codec::BuffaCodec`

## Important Options

- `build_client(bool)`: enable or disable client stub generation
- `build_server(bool)`: enable or disable server stub generation
- `build_transport(bool)`: forward tonic transport generation settings
- `codec_path(...)`: set the codec used by generated methods
- `extern_path(proto, rust)`: map protobuf packages or types onto existing Rust
  paths; longest prefix match wins
- `include_file(...)`: write a module include file in addition to per-package
  files
- `out_dir(...)`: write generated files somewhere other than `OUT_DIR`
- `compile_well_known_types(bool)`: forward tonic handling for protobuf
  well-known types
- `emit_package(bool)`: control tonic package module emission
- `compile_protos(...)`: run `protoc`, load a descriptor set, and generate code
- `compile_fds(...)`: generate directly from an existing descriptor set

## View Mode

`ServerRequestMode::Owned` is the default. If you switch to
`ServerRequestMode::View`, generated server traits use a wrapper type around the
request message. If you use view mode outside a setup that already provides the
default wrapper type, set `view_wrapper_path(...)` explicitly.

## Output

Each protobuf package is written to its own file. For example, package
`hello.v1` becomes `hello.v1.rs`. If you set `include_file("mod.rs")`, the
crate also generates a module tree that re-exports those package files with
`include!(...)`.

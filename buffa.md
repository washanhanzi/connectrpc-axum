Context
Replace prost with buffa as the protobuf runtime. buffa offers zero-copy views, built-in JSON (eliminating pbjson), better enum/field ergonomics, and linear-time serialization. Keep tonic working via impl prost::Message bridge on buffa-generated types. Keep prost-build only for descriptor generation and service generator hook (buffa-build has neither).
This is a breaking change for downstream users (trait bounds, re-exports, generated field shapes all change).
New Build Pipeline
Pass 1:  prost-build → descriptor.bin only (temp dir, discard .rs files)
Pass 2:  buffa-build → message types + serde JSON (replaces prost messages + pbjson)
Pass 3:  Parse descriptor.bin → extract services → run refactored AxumConnectServiceGenerator
Pass 4:  [tonic] Generate `impl prost::Message for BufaType` per message type
Pass 5:  [tonic] tonic-prost-build → server stubs (uses descriptor.bin + extern_path)
Pass 6:  [tonic-client] tonic-prost-build → client stubs
Cleanup: delete descriptor.bin
Implementation Steps
Step 0: Create buffa.md reference document and commit to new branch
Create buffa.md in the project root documenting what we've learned about buffa's API for future reference:

buffa v0.2.0 on crates.io: buffa, buffa-build, buffa-types, buffa-codegen
Message trait: compute_size(), write_to(), encode_to_vec(), decode_from_slice(), merge_field(tag, buf, depth), cached_size(), clear()
Supertraits: DefaultInstance + Clone + PartialEq + Send + Sync
Tag: Tag::new(field_number: u32, wire_type: WireType)
WireType: Varint=0, Fixed64=1, LengthDelimited=2, StartGroup=3, EndGroup=4, Fixed32=5 (identical values to prost)
buffa-build Config: files(), includes(), out_dir(), generate_json(), generate_views(), extern_path(), include_file(), descriptor_set(), compile()
Key differences from prost: two-pass serialization (compute_size then write_to), DefaultInstance instead of Default, decode_from_slice() instead of decode(), Tag instead of separate (u32, WireType)

Commit to the claude/jolly-mccarthy branch.
Step 1: Define own Service/Method descriptor types
File: connectrpc-axum-build/src/descriptor.rs (new)
Create standalone types replacing prost_build::Service and prost_build::Method:
rustpub struct ServiceDescriptor {
    pub name: String,        // "HelloWorldService"
    pub proto_name: String,  // "HelloWorldService"
    pub package: String,     // "hello"
    pub methods: Vec<MethodDescriptor>,
}

pub struct MethodDescriptor {
    pub name: String,        // snake_case
    pub proto_name: String,  // original CamelCase
    pub input_type: String,  // Rust path "super::HelloRequest"
    pub output_type: String,
    pub client_streaming: bool,
    pub server_streaming: bool,
    pub idempotency_level: Option<i32>,
}
Add a function parse_services(descriptor_bytes: &[u8]) -> Vec<ServiceDescriptor> that reads FileDescriptorSet and extracts service metadata. Use prost_types::FileDescriptorSet for parsing (we keep prost-types as a dep).
Step 2: Refactor service generator off prost_build types
Files:

connectrpc-axum-build/src/gen.rs — change AxumConnectServiceGenerator::generate() to accept &ServiceDescriptor instead of prost_build::Service. Remove impl ServiceGenerator for AxumConnectServiceGenerator.
connectrpc-axum-build/src/gen/tonic.rs — change generate_tonic_code() to accept &ServiceDescriptor instead of &prost_build::Service
connectrpc-axum-build/src/gen/client.rs — change generate_connect_client() to accept &ServiceDescriptor
connectrpc-axum-build/src/gen/tests.rs — rewrite test fixtures to use ServiceDescriptor/MethodDescriptor

The MethodInfo tuple type stays the same — it's built from our descriptor types now.
Step 3: Create prost::Message bridge generator
File: connectrpc-axum-build/src/gen/prost_bridge.rs (new)
Takes list of message type paths (from descriptor), generates per-type:
rustimpl ::prost::Message for #type_path {
    fn encode_raw(&self, buf: &mut impl ::bytes::BufMut) {
        ::buffa::Message::compute_size(self); // ensure cached
        ::buffa::Message::write_to(self, buf);
    }
    fn merge_field(&mut self, tag: u32, wire_type: ::prost::encoding::WireType,
                   buf: &mut impl ::bytes::Buf, ctx: ::prost::encoding::DecodeContext
    ) -> ::core::result::Result<(), ::prost::DecodeError> {
        // wire type value mapping (both use same 3-bit protobuf encoding)
        let buffa_tag = ::buffa::encoding::Tag::new(tag, /* convert wire_type */);
        ::buffa::Message::merge_field(self, buffa_tag, buf, ctx.remaining_depth())
            .map_err(|e| ::prost::DecodeError::new(e.to_string()))
    }
    fn encoded_len(&self) -> usize {
        ::buffa::Message::compute_size(self) as usize
    }
    fn clear(&mut self) {
        ::buffa::Message::clear(self);
    }
}
Step 4: Rewrite compile_source()
File: connectrpc-axum-build/src/lib.rs
Replace the current 4-pass pipeline with the new 6-pass pipeline described above.
Key changes:

Pass 1: Use prost_build::Config to generate descriptor.bin only (compile to temp dir, keep only descriptor.bin)
Pass 2: buffa_build::Config::new().files().includes().generate_json(true).compile()
Pass 3: Parse descriptor.bin → Vec<ServiceDescriptor> → generate service code → append to buffa output files
Pass 4: Generate prost::Message bridge impls (gated on tonic feature)
Pass 5/6: tonic-prost-build passes (unchanged logic)

Remove generate_pbjson() and the .serde.rs file merging logic entirely.
Step 5: Update CompileBuilder API
File: connectrpc-axum-build/src/lib.rs

Replace prost_config: Option<Box<dyn Fn(&mut prost_build::Config)>> with buffa_config: Option<Box<dyn Fn(&mut buffa_build::Config)>>
Remove pbjson_config field and with_pbjson_config() method
Rename with_prost_config() to with_buffa_config()
Keep tonic_config / tonic_client_config unchanged

Step 6: Update include_file.rs
File: connectrpc-axum-build/src/include_file.rs

Change extern reexport from ::pbjson_types::* to ::buffa_types::google::protobuf::* for google.protobuf package
Remove .serde.rs file filtering (buffa generates serde inline)

Step 7: Update connectrpc-axum-build Cargo.toml
File: connectrpc-axum-build/Cargo.toml

Add: buffa-build
Keep: prost-build (descriptor generation), prost-types (descriptor parsing), prost (bridge codegen references)
Remove: pbjson-build

Step 8: Migrate connectrpc-axum runtime
Files:

connectrpc-axum/Cargo.toml: replace prost → buffa, remove pbjson/pbjson-types, add buffa-types
connectrpc-axum/src/lib.rs:100-104: pub use buffa; pub use buffa_types; (remove pbjson re-exports)
connectrpc-axum/src/handler.rs:17,114,171: prost::Message → buffa::Message
connectrpc-axum/src/message/request.rs:23,86: prost::Message → buffa::Message, T::decode(bytes) → T::decode_from_slice(bytes)
connectrpc-axum/src/message/response.rs:24: prost::Message → buffa::Message (encode_to_vec stays same)
connectrpc-axum/src/tonic/handler.rs:21,552,593,642,687: prost::Message → buffa::Message

Step 9: Migrate connectrpc-axum-client
Files:

connectrpc-axum-client/Cargo.toml: replace prost → buffa
src/client.rs:21: prost::Message → buffa::Message
src/request/encoder.rs:18,124: change import + encoded_len() → compute_size() as usize
src/response/decoder.rs:20: change import
src/config/interceptor.rs:49: change import
src/response/intercepting.rs:13: change import
Test impl prost::Message for TestMessage blocks (encoder.rs:258, decoder.rs:408, streaming.rs:281, interceptor.rs:1052): rewrite as impl buffa::Message for TestMessage

Step 10: Remove unused prost from connectrpc-axum-core
File: connectrpc-axum-core/Cargo.toml:13 — remove prost (confirmed unused in source)
Step 11: Migrate test and example crates
Files:

connectrpc-axum-test/Cargo.toml: prost → buffa, prost-types → buffa-types, remove pbjson deps
connectrpc-axum-examples/Cargo.toml: same pattern
Source files: use prost::Message → use buffa::Message, pbjson_types::* → buffa_types::google::protobuf::*

Step 12: Update root workspace Cargo.toml
File: Cargo.toml

Add: buffa = "0.2", buffa-build = "0.2", buffa-types = "0.2"
Keep: prost, prost-build, prost-types (needed for tonic + descriptor gen)
Remove: pbjson, pbjson-build, pbjson-types

Risks

compute_size() before write_to() in bridge: prost's encode() calls encoded_len() then encode_raw(), so compute_size() is always called first. Defensive: call compute_size() inside encode_raw() too.
buffa wire type enum values: Must verify buffa's WireType variants map 1:1 to prost's. Both follow the protobuf spec so they should match, but verify the exact enum names/values.
buffa-build output file naming: Must match what include_file.rs expects. buffa uses dot-separated names like hello.rs — verify compatibility.
Default trait: prost requires Default for decode. buffa uses DefaultInstance. buffa types should also impl Default — verify this.

Verification

cargo build — all crates compile
cargo build --features tonic — tonic feature compiles with bridge impls
cargo test — all existing tests pass
Run integration tests in connectrpc-axum-test/
Build and run examples in connectrpc-axum-examples/
Verify generated code: inspect OUT_DIR files for correct buffa types + service stubs + prost bridge impls
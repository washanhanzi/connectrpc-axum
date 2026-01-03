# Plan: connectrpc-errordetails Subcrate

Create a new subcrate `connectrpc-errordetails` that provides ergonomic error detail handling for ConnectRPC, similar to Go's `google.golang.org/genproto/googleapis/rpc/errdetails` package.

## Problem

Currently, adding error details in Rust requires manual protobuf encoding:

```rust
// Current: manual encoding (error-prone, verbose)
fn encode_retry_info(seconds: i64) -> Vec<u8> {
    let duration = prost_types::Duration { seconds, nanos: 0 };
    let mut duration_bytes = Vec::new();
    duration.encode(&mut duration_bytes).unwrap();

    let mut bytes = Vec::new();
    bytes.push(0x0a); // field 1, wire type 2
    bytes.push(duration_bytes.len() as u8);
    bytes.extend(duration_bytes);
    bytes
}

Err(ConnectError::new(Code::ResourceExhausted, "rate limited")
    .add_detail("google.rpc.RetryInfo", encode_retry_info(5)))
```

In Go, it's simple:

```go
import "google.golang.org/genproto/googleapis/rpc/errdetails"

retryInfo := &errdetails.RetryInfo{
    RetryDelay: &durationpb.Duration{Seconds: 5},
}
detail, _ := connect.NewErrorDetail(retryInfo)
err.AddDetail(detail)
```

## Solution

Create `connectrpc-errordetails` subcrate that:

1. **Pre-generates `google.rpc` error detail types** from official protos
2. **Enables `Name` trait** for type URL discovery via `prost_types::Any`
3. **Provides helper methods** for encoding/decoding details

## Implementation Steps

### 1. Create subcrate structure

```
connectrpc-errordetails/
├── Cargo.toml
├── build.rs
├── proto/
│   └── google/
│       └── rpc/
│           ├── error_details.proto
│           └── status.proto
└── src/
    └── lib.rs
```

### 2. Add proto files

Download from googleapis:
- `google/rpc/error_details.proto` - defines RetryInfo, DebugInfo, QuotaFailure, etc.
- `google/rpc/status.proto` - defines Status message
- `google/protobuf/duration.proto` - dependency (use prost-types)
- `google/protobuf/any.proto` - dependency (use prost-types)

### 3. Configure build.rs

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = prost_build::Config::new();

    // Enable Name trait for Any support
    config.enable_type_names();

    // Use prost-types for well-known types
    config.extern_path(".google.protobuf.Duration", "::prost_types::Duration");
    config.extern_path(".google.protobuf.Any", "::prost_types::Any");

    config.compile_protos(
        &["proto/google/rpc/error_details.proto"],
        &["proto"],
    )?;

    Ok(())
}
```

### 4. Generated types

The crate will provide these types (from `google/rpc/error_details.proto`):

| Type | Description | Common Use |
|------|-------------|------------|
| `RetryInfo` | When to retry | Rate limiting, temporary failures |
| `DebugInfo` | Debug information | Stack traces, diagnostic info |
| `QuotaFailure` | Quota violations | Resource exhaustion details |
| `ErrorInfo` | Structured error info | Error domain, reason, metadata |
| `PreconditionFailure` | Precondition violations | Failed preconditions list |
| `BadRequest` | Request field violations | Validation errors |
| `RequestInfo` | Request identification | Request ID, serving data |
| `ResourceInfo` | Resource information | Resource type, name, owner |
| `Help` | Help links | Documentation links |
| `LocalizedMessage` | Localized messages | i18n error messages |

### 5. Helper trait for ConnectError

```rust
// In connectrpc-errordetails/src/lib.rs

use prost::{Message, Name};
use prost_types::Any;

/// Extension trait for adding typed error details
pub trait ErrorDetailExt {
    /// Add a typed error detail (auto-discovers type URL via Name trait)
    fn add_error_detail<M: Message + Name>(self, detail: &M) -> Self;

    /// Try to extract a typed error detail
    fn get_error_detail<M: Message + Name + Default>(&self) -> Option<M>;
}

impl ErrorDetailExt for connectrpc_axum::ConnectError {
    fn add_error_detail<M: Message + Name>(self, detail: &M) -> Self {
        let type_name = M::full_name();
        let bytes = detail.encode_to_vec();
        self.add_detail(&type_name, bytes)
    }

    fn get_error_detail<M: Message + Name + Default>(&self) -> Option<M> {
        let type_name = M::full_name();
        self.details()
            .iter()
            .find(|d| d.type_name == type_name)
            .and_then(|d| M::decode(d.value.as_slice()).ok())
    }
}
```

### 6. Convenience constructors

```rust
// Convenience functions for common patterns

impl RetryInfo {
    /// Create RetryInfo with seconds delay
    pub fn with_delay_secs(seconds: i64) -> Self {
        Self {
            retry_delay: Some(prost_types::Duration { seconds, nanos: 0 }),
        }
    }

    /// Create RetryInfo with Duration
    pub fn with_delay(delay: std::time::Duration) -> Self {
        Self {
            retry_delay: Some(prost_types::Duration {
                seconds: delay.as_secs() as i64,
                nanos: delay.subsec_nanos() as i32,
            }),
        }
    }
}

impl BadRequest {
    /// Create BadRequest with a single field violation
    pub fn field_violation(field: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            field_violations: vec![bad_request::FieldViolation {
                field: field.into(),
                description: description.into(),
            }],
        }
    }
}

impl ErrorInfo {
    /// Create ErrorInfo with reason and domain
    pub fn new(reason: impl Into<String>, domain: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            domain: domain.into(),
            metadata: Default::default(),
        }
    }
}
```

## Usage Example (After Implementation)

```rust
use connectrpc_axum::prelude::*;
use connectrpc_errordetails::{ErrorDetailExt, RetryInfo, BadRequest};

async fn handler(req: ConnectRequest<MyRequest>) -> Result<ConnectResponse<MyResponse>, ConnectError> {
    // Rate limiting with retry info
    if rate_limited() {
        return Err(
            ConnectError::new(Code::ResourceExhausted, "rate limited")
                .add_error_detail(&RetryInfo::with_delay_secs(30))
        );
    }

    // Validation error with field violations
    if req.name.is_empty() {
        return Err(
            ConnectError::new(Code::InvalidArgument, "validation failed")
                .add_error_detail(&BadRequest::field_violation("name", "name is required"))
        );
    }

    Ok(ConnectResponse::new(MyResponse { ... }))
}
```

## Cargo.toml

```toml
[package]
name = "connectrpc-errordetails"
version = "0.1.0"
edition = "2021"
description = "Google RPC error detail types for ConnectRPC"
license = "MIT OR Apache-2.0"
repository = "https://github.com/..."

[dependencies]
prost = "0.13"
prost-types = "0.13"
connectrpc-axum = { version = "0.0.13", path = "../connectrpc-axum" }

[build-dependencies]
prost-build = "0.13"
```

## Testing

1. Unit tests for each error detail type encoding/decoding
2. Integration test with Go client (extend `error_details_test.go`)
3. Roundtrip tests: Rust encode → Go decode, Go encode → Rust decode

## Open Questions

1. **Crate name**: `connectrpc-errordetails` vs `connectrpc-rpc-status` vs `connectrpc-errdetails`?
2. **Feature flag**: Should `ErrorDetailExt` trait be behind a feature flag in `connectrpc-axum`?
3. **Re-export**: Should `connectrpc-axum` re-export common types from this crate?

## References

- [google/rpc/error_details.proto](https://github.com/googleapis/googleapis/blob/master/google/rpc/error_details.proto)
- [Go errdetails package](https://pkg.go.dev/google.golang.org/genproto/googleapis/rpc/errdetails)
- [Connect protocol error details spec](https://connectrpc.com/docs/protocol/#error-end-stream)
- [prost Name trait](https://docs.rs/prost-types/latest/prost_types/struct.Any.html)

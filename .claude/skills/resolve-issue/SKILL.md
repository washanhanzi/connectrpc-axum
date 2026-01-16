---
name: resolve-issue
description: Investigate and resolve GitHub issues for connectrpc-axum. Requires a GitHub issue link or number. Analyzes the issue, references architecture docs and connect-go implementation, creates integration tests if needed, then posts a resolution plan or answer.
---

# Resolve Issue

Investigate and resolve GitHub issues for the connectrpc-axum project.

## Requirements

**A GitHub issue link or number is required.** Examples:
- `#42`
- `42`
- `https://github.com/frankgreco/connectrpc-axum/issues/42`

## Workflow

### 1. Fetch Issue Details

Get the issue content using `gh`:

```bash
gh issue view <number> --repo frankgreco/connectrpc-axum
```

Identify the issue type:
- **Question**: User needs help understanding something
- **Bug Report**: Something isn't working as expected
- **Feature Request**: New functionality requested

### 2. Research Phase

#### 2a. Check Project Documentation

Read relevant docs based on the issue topic:

1. `docs/guide/architecture.md` - Module structure, request flow, key types
2. `docs/guide/index.md` - Features and capabilities
3. Topic-specific guides in `docs/guide/`:
   - `configuration.md` - Service/handler config
   - `compression.md` - Compression support
   - `timeout.md` - Timeout handling
   - `tonic.md` - Tonic integration
   - `grpc-web.md` - gRPC-Web support

#### 2b. Check connect-go Reference

Use the `connect-go-reference` skill to verify protocol behavior:

```bash
# Search local connect-go/ directory
Grep pattern="<relevant-pattern>" path="connect-go/"
Read file_path="connect-go/<relevant-file>.go"
```

Key files:
- `protocol_connect.go` - Connect protocol implementation
- `protocol_grpc.go` - gRPC protocol
- `error.go` - Error handling
- `envelope.go` - Streaming frame format

**NEVER use WebFetch/WebSearch for connect-go - always use local files.**

#### 2c. Search Codebase

Search the project for relevant code:

```bash
# Find related implementations
Grep pattern="<keyword>" path="connectrpc-axum/"
Grep pattern="<keyword>" path="connectrpc-axum-build/"
```

### 3. For Bugs - Reproduce and Verify

#### 3a. Run Existing Tests

First, check if existing tests cover the issue:

```bash
# Run unit tests
cargo test

# Run integration tests
go test -C connectrpc-axum-examples/go-client -v -timeout 300s
```

#### 3b. Create Reproduction Test

If the bug isn't covered by existing tests, create an integration test:

1. **Create Rust server** in `connectrpc-axum-examples/src/bin/<issue-name>.rs`:

```rust
use axum::Router;
use connectrpc_axum::prelude::*;
use tokio::net::TcpListener;

// Import generated code
use connectrpc_axum_examples::hello::*;

#[tokio::main]
async fn main() {
    let app = Router::new()
        .connect_service(HelloWorldServiceBuilder::new()
            .say_hello(|req: ConnectRequest<HelloRequest>| async move {
                // Handler implementation
                Ok(HelloResponse {
                    message: format!("Hello, {}!", req.message.name.unwrap_or_default()),
                })
            })
        );

    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Server ready on port 3000");
    axum::serve(listener, app).await.unwrap();
}
```

2. **Create Go test** in `connectrpc-axum-examples/go-client/<issue-name>_test.go`:

```go
package main

import (
    "context"
    "net/http"
    "testing"

    "connectrpc.com/connect"
    "github.com/connectrpc-axum/examples/go-client/gen"
    "github.com/connectrpc-axum/examples/go-client/gen/genconnect"
)

func TestIssue42(t *testing.T) {
    s := startServer(t, "<issue-name>", "")
    defer s.stop()

    client := genconnect.NewHelloWorldServiceClient(
        http.DefaultClient,
        serverURL,
    )

    // Test the specific scenario from the issue
    resp, err := client.SayHello(context.Background(), connect.NewRequest(&gen.HelloRequest{
        Name: proto.String("test"),
    }))
    if err != nil {
        t.Fatalf("RPC failed: %v", err)
    }

    // Assert expected behavior
    if resp.Msg.Message != "expected" {
        t.Errorf("got %q, want %q", resp.Msg.Message, "expected")
    }
}
```

3. **Run the specific test**:

```bash
go test -C connectrpc-axum-examples/go-client -v -run TestIssue42
```

### 4. Formulate Resolution

Based on findings, prepare one of:

#### For Questions
- Write a clear answer with code examples
- Reference relevant documentation
- Suggest documentation improvements if the answer wasn't obvious

#### For Bugs
- Document the root cause
- Create a fix plan with specific files and changes
- Include test cases that verify the fix

#### For Feature Requests
- Assess feasibility based on architecture
- Outline implementation approach
- Identify affected modules and files
- Note any connect-go reference patterns to follow

### 5. Post Response to GitHub

Use `gh` to comment on the issue:

```bash
gh issue comment <number> --repo frankgreco/connectrpc-axum --body "$(cat <<'EOF'
## Investigation Summary

<summary of what was found>

## Root Cause / Answer

<explanation>

## Resolution Plan

<if applicable: specific steps to fix>

## References

- `docs/guide/<relevant>.md`
- `connect-go/<file>.go` (lines X-Y)
- `connectrpc-axum/src/<file>.rs` (lines X-Y)

---
*Investigated by Claude Code*
EOF
)"
```

### 6. For Fixes - Create Implementation Plan

If fixing the issue, outline:

1. **Files to modify** with specific changes
2. **New tests** to add
3. **Documentation updates** if needed

Then either:
- Implement the fix directly (for small changes)
- Create a detailed plan for user approval (for larger changes)

## Reference Skills

- **connect-go-reference**: Verify protocol behavior against Go implementation
- **test**: Run the complete test suite
- **architecture**: Understand project structure and design

## Integration Test Patterns

Existing test patterns in `connectrpc-axum-examples/go-client/`:

| File | Tests |
|------|-------|
| `unary_test.go` | Basic unary RPC |
| `server_stream_test.go` | Server streaming |
| `bidi_stream_test.go` | Bidirectional streaming |
| `timeout_test.go` | Connect-Timeout-Ms |
| `error_details_test.go` | Error detail encoding |
| `compression_test.go` | Compression negotiation |
| `grpc_web_test.go` | gRPC-Web protocol |

Use these as templates for new reproduction tests.

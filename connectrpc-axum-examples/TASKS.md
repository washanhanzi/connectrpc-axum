# ConnectRPC Axum Examples - Task Reference

Quick reference for `cargo-make` tasks. Run from the `connectrpc-axum-examples/` directory.

## Quick Start

```bash
cargo make setup          # One-time setup
cargo make run-connect-only    # Terminal 1
cargo make go-run              # Terminal 2
```

## Server Tasks

| Task | Description |
|------|-------------|
| `cargo make run-connect-only` | Run pure Connect server (port 3000) |
| `cargo make run-connect-tonic` | Run Connect + Tonic server |
| `cargo make run-connect-tonic-bidi` | Run server with bidi streaming |
| `cargo make build-servers` | Build all server binaries |
| `cargo make watch-server` | Auto-restart on code changes |

## Go Client Tasks

| Task | Description |
|------|-------------|
| `cargo make go-generate` | Generate protobuf code from .proto files |
| `cargo make go-build` | Build the streaming test client |
| `cargo make go-run` | Run the streaming test client |
| `cargo make go-deps` | Download Go dependencies |
| `cargo make go-clean` | Remove generated files |

## Build & Maintenance

| Task | Description |
|------|-------------|
| `cargo make build-all` | Build all servers + Go client |
| `cargo make clean-all` | Clean all build artifacts |
| `cargo make setup` | Initial setup (run once) |
| `cargo make help` | Show detailed help |

## Common Workflows

### First Time Setup
```bash
cargo make setup
```

### Run Server and Test
```bash
# Terminal 1
cargo make run-connect-only

# Terminal 2
cargo make go-run
```

### Development Loop
```bash
# Auto-rebuild and restart on changes
cargo make watch-server

# In another terminal
cargo make go-run
```

### Test Different Servers
```bash
# Test each server implementation
cargo make run-connect-only
cargo make run-connect-tonic
cargo make run-connect-tonic-bidi

# Each time, run in another terminal:
cargo make go-run
```

### Clean and Rebuild
```bash
cargo make clean-all
cargo make build-all
```

## File Locations

- **Makefile.toml**: Task definitions
- **proto/**: Protocol Buffer definitions
- **src/bin/**: Server implementations
- **go-client/**: Go test client

## Tips

- Use `cargo make --list-all-steps` to see all tasks with categories
- Tasks can be run from any directory by specifying paths
- The Go client tests Connect protocol conformance
- All servers run on port 3000 by default

## Troubleshooting

**Port in use:**
```bash
lsof -ti:3000 | xargs kill -9
```

**Missing buf CLI:**
```bash
brew install bufbuild/buf/buf
```

**Missing cargo-make:**
```bash
cargo install cargo-make
```

**Go module errors:**
```bash
cargo make go-clean
cargo make go-generate
```

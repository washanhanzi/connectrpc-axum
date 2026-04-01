# Benchmarks

This crate contains two runnable unary benchmark suites:

- `protocol_benchmarks`: in-memory framework/protocol overhead without Valkey
- `app_benchmarks`: app-shaped Fortune RPCs with Valkey still in the hot path

All benchmark cases use the same generated `connect-go` client implementation. The Rust runner only builds targets, starts servers, executes the case matrix, prints readable tables, and writes machine-readable JSON results.

## Suites

### `protocol_benchmarks`

Purpose: compare protocol and framework overhead with a shared unary echo workload.

Characteristics:

- unary only
- no Valkey
- in-memory handler
- request payload sizes: `small`, `medium`, `large`
- compression: `identity`, `gzip`
- protocols: `connect_json`, `connect_protobuf`, `grpc`

### `app_benchmarks`

Purpose: compare app-shaped behavior with backend work still present.

Characteristics:

- unary only
- keeps `FortuneService/GetFortunes`
- keeps Valkey in the hot path
- request size varies through repeated fields in `GetFortunesRequest`
- request payload sizes: `small`, `medium`, `large`
- compression: `identity`, `gzip`
- protocols: `connect_json`, `connect_protobuf`, `grpc`

## Targets

Explicit local targets:

- `connectrpc_axum`
- `connect_rust`
- `tonic`
- `connect_go`

Constraint:

- `tonic` participates in `grpc` only

## Case Matrix

Benchmark case names are target-first:

- `target_protocol_payload_compression`

Examples:

- `connectrpc_axum_connect_json_small_identity`
- `connect_rust_connect_protobuf_large_gzip`
- `tonic_grpc_medium_gzip`

Per suite:

- `connectrpc_axum`: 18 cases
- `connect_rust`: 18 cases
- `connect_go`: 18 cases
- `tonic`: 6 cases

Total: 60 cases per suite before concurrency levels.

## Commands

Generate Go stubs after editing `proto/`:

```bash
./benchmarks/generate-go.sh
```

Run both suites with the default concurrency set:

```bash
cargo run -p connectrpc-axum-benchmarks --release --bin benchmarks
```

Run a quick protocol-only pass:

```bash
cargo run -p connectrpc-axum-benchmarks --release --bin benchmarks -- --quick --suite=protocol
```

Run a single filtered case while iterating:

```bash
cargo run -p connectrpc-axum-benchmarks --release --bin benchmarks -- --quick --suite=app --target=connectrpc_axum --case-filter=connectrpc_axum_connect_protobuf_small_identity
```

Use the higher concurrency set:

```bash
cargo run -p connectrpc-axum-benchmarks --release --bin benchmarks -- --high-c
```

## Output

Readable tables are printed to stdout for each suite.

Machine-readable JSON is written to:

```text
target/benchmarks/results/latest.json
```

Override the JSON path with:

```bash
--json-out=/path/to/results.json
```

## Notes

- The local `connect-rust` target is built from in-repo benchmark server sources using pinned git dependencies from `anthropics/connect-rust` commit `e3fafcb94fc14daf970224f0eff2ba597c71ae47`.
- `connect-go` codegen stays under `benchmarks/connect-go/` via `buf`, and the benchmark Go module uses the repo-local root [connect-go](/home/frank/github/connectrpc-axum/connect-go) runtime via `replace`.
- `app_benchmarks` needs Valkey. By default the runner starts `valkey/valkey:8-alpine` through Docker.
- Set `BENCHMARKS_VALKEY_ADDR=host:port` to reuse an existing Valkey instance. `FORTUNE_VALKEY_ADDR` is still accepted as a compatibility fallback.

# Benchmark: `buffa` vs `0.1.0` vs `connect-rust`

This workspace compares:

- The current local `buffa` branch checkout from this repository
- The published `connectrpc-axum@0.1.0` and `connectrpc-axum-build@0.1.0` crates from crates.io
- The published `connectrpc@0.2.1` and `connectrpc-build@0.2.0` crates from the `connect-rust` project, which are also Buffa-backed

The benchmark workspace directory still carries its original `compare-buffa-beta5`
name, but the published `connectrpc-axum` comparison target is now the released
`0.1.0` crates.

The benchmark suite now focuses on a Fortune-style unary workload over loopback TCP:

- A real Valkey-backed Fortune service in each target
- A shared load generator that drives concurrent Connect unary proto requests
- A second shared load generator that drives concurrent Connect unary JSON requests through `reqwest`
- Timed throughput and latency output (`req/s`, `p50`, `p99`)
- Code generation timing via a separate script

## Layout

- `cases-buffa/`: helper crate built against the local checkout
- `cases-release/`: helper crate built against the published `0.1.0` release crates
- `cases-connectrpc/`: helper crate built against the published `connect-rust` crates
- `common/`: shared Fortune and Valkey helpers
- `runner/`: custom Fortune benchmark runner
- `proto/`: shared benchmark proto fixtures
- `results/`: checked-in benchmark summaries
- `scripts/bench_codegen.sh`: codegen timing helper

## Run

From this directory:

```bash
cargo bench -p compare-buffa-beta5-runner --bench fortunes
```

Quick mode:

```bash
cargo bench -p compare-buffa-beta5-runner --bench fortunes -- --quick
```

Specific concurrency levels:

```bash
cargo bench -p compare-buffa-beta5-runner --bench fortunes -- --concurrency=16,64
```

## Notes

- The current setup compares the local checkout against the published `0.1.0` release crates from crates.io and the published `connect-rust` crates.
- The current suite removes the older synthetic `hello` RPC benchmark and replaces it with a Valkey-backed Fortune workload.
- The runner uses the generated `connect-rust` Fortune client as the uniform Connect client for the proto benchmark rows, matching the upstream `fortune_bench` load loop more closely.
- The JSON benchmark rows use `reqwest` to send `application/json` Connect unary requests to all three servers.
- The `connect-rust` target now runs through its native `connectrpc::server::Server` path instead of the earlier Axum fallback wrapper.
- Each target still runs as a real loopback TCP server backed by the same Valkey dataset.
- If `VALKEY_ADDR` is unset, the runner starts a disposable `valkey/valkey:8-alpine` container through `docker`, seeds it, and tears it down on exit.
- If `VALKEY_ADDR` is set, the runner uses that existing Valkey instance instead.
- The output is a markdown table printed to stdout, not Criterion JSON or HTML.

## Codegen Timing

```bash
./scripts/bench_codegen.sh
```

If `hyperfine` is installed, the script uses it automatically. Otherwise it falls back to `time -p`.

## Current Conclusion

See `results/2026-03-26-initial.md` and `results/2026-03-26-conclusion.md` for the older synthetic benchmark notes, `results/2026-03-26-fortunes.md` for the first Fortune-harness pass, `results/2026-03-27-fortunes-aligned.md` for the aligned proto-only Fortune run, and `results/2026-03-27-fortunes-proto-json.md` for the current proto+json Fortune run.

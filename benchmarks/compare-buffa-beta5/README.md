# Benchmark: `buffa` vs `0.1.0` vs `connect-rust`

This workspace compares:

- The current local `buffa` branch checkout from this repository
- The published `connectrpc-axum@0.1.0` and `connectrpc-axum-build@0.1.0` crates from crates.io
- The published `connectrpc@0.2.1` and `connectrpc-build@0.2.0` crates from the `connect-rust` project, which are also Buffa-backed

The benchmark workspace directory still carries its original `compare-buffa-beta5`
name, but the published `connectrpc-axum` comparison target is now the released
`0.1.0` crates.

The benchmark suite focuses on framework-level round trips over loopback TCP:

- Loopback Connect unary proto round trips
- Loopback Connect unary JSON round trips
- Loopback Connect server-streaming proto round trips
- Code generation timing via a separate script

## Layout

- `cases-buffa/`: helper crate built against the local checkout
- `cases-release/`: helper crate built against the published `0.1.0` release crates
- `cases-connectrpc/`: helper crate built against the published `connect-rust` crates
- `runner/`: Criterion benchmark runner
- `proto/`: shared benchmark proto fixtures
- `results/`: checked-in benchmark summaries
- `scripts/bench_codegen.sh`: codegen timing helper
- `scripts/summarize_results.sh`: Criterion result summary helper

## Run

From this directory:

```bash
cargo bench -p compare-buffa-beta5-runner
```

To run a subset:

```bash
cargo bench -p compare-buffa-beta5-runner --bench compare -- 'connect_unary_proto_roundtrip'
```

Criterion HTML output will be written under:

```text
target/criterion/
```

## Notes

- The current setup compares the local checkout against the published `0.1.0` release crates from crates.io and the published `connect-rust` crates.
- The Criterion suite intentionally excludes standalone encode/decode microbenchmarks so the reported numbers stay focused on framework-level RPC behavior.
- Each target runs as a real loopback TCP server, and the runner sends a common wire-format request through the same HTTP client for all three targets.
- The runner redirects `stderr` to `/dev/null` while executing `connect_stream_proto_roundtrip` so the runtimes' per-message debug output does not distort those measurements.

## Codegen Timing

```bash
./scripts/bench_codegen.sh
```

If `hyperfine` is installed, the script uses it automatically. Otherwise it falls back to `time -p`.

## Summarize Results

```bash
./scripts/summarize_results.sh
```

The summary script reads Criterion `estimates.json` files and prints a markdown table.
Positive `Buffa delta vs 0.1.0` and `Connect-rust delta vs 0.1.0` values mean those targets are faster than `0.1.0`.

## Current Conclusion

See `results/2026-03-26-initial.md` and `results/2026-03-26-conclusion.md` for the older pre-`0.1.0` / pre-`connect-rust` measurements and decision note. Rerun the benchmark workspace after this change to collect updated three-way results.

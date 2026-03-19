# Benchmark: `buffa` vs `0.1.0-beta.5`

This workspace compares:

- The current local `buffa` branch checkout from this repository
- The published `connectrpc-axum@0.1.0-beta.5` and `connectrpc-axum-build@0.1.0-beta.5` crates from crates.io

The repository already contains packaged beta.5 sources under `target/package/`, but
Cargo cannot use them alongside the local `buffa` checkout in a single lockfile because
both path sources share the same package name and version. Using the published release
avoids that lockfile collision while keeping the comparison target equivalent.

The first benchmark pass covers:

- Protobuf encode/decode
- JSON encode/decode
- In-process Connect unary round trips
- In-process Connect server-streaming round trips
- Code generation timing via a separate script

## Layout

- `cases-buffa/`: helper crate built against the local checkout
- `cases-beta5/`: helper crate built against the published `0.1.0-beta.5` crates
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
cargo bench -p compare-buffa-beta5-runner --bench compare -- 'proto_encode_hello_request'
```

Criterion HTML output will be written under:

```text
target/criterion/
```

## Notes

- The current setup compares the local checkout against the published `0.1.0-beta.5` crates from crates.io. This is equivalent to the packaged release target for benchmark purposes and avoids Cargo lockfile collisions.
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
A positive `Buffa delta` means the local `buffa` target is faster than beta.5.

## Current Conclusion

See `results/2026-03-26-initial.md` for the measured numbers and
`results/2026-03-26-conclusion.md` for the current decision note.

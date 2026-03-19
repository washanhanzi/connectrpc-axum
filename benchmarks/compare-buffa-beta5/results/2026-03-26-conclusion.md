# Conclusion: 2026-03-26

## Decision

Do not justify the `buffa` migration on measured performance alone.

## Why

- The largest end-to-end win in the current harness was the medium `connect_unary_proto_roundtrip` case at `17.0%`, but that was only `753.867 ns` (`0.754 us`) faster per in-process round trip.
- Other end-to-end wins were smaller, mostly in the `1%` to `11%` range.
- Pure protobuf encode regressed versus `0.1.0-beta.5`, especially for the large payload.
- These benchmarks intentionally remove network and application noise, so they should be treated as framework-overhead upper bounds rather than production latency wins.

## Current Read

The local `buffa` branch is not obviously faster in a way that alone justifies a migration of this size. Any decision to continue should be driven by API direction, zero-copy/view semantics, or maintenance strategy rather than the benchmark numbers collected so far.

## If Revisited Later

- Add a real localhost client benchmark instead of only in-process server-path benches.
- Capture allocation counts or profiles alongside wall-clock time.
- Re-run on a representative production-class machine and workload mix.
- Re-evaluate if the project wants `buffa` for reasons other than raw throughput.

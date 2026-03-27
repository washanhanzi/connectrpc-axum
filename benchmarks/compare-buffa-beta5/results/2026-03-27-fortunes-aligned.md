# Fortune Benchmark (Aligned Harness): 2026-03-27

## Environment

- Host: `Linux ravenz 6.19.6-arch1-1 x86_64`
- CPU: `AMD RYZEN AI MAX+ PRO 395 w/ Radeon 8060S`
- `rustc`: `1.94.0 (4a4ef493e 2026-03-02)`
- `cargo`: `1.94.0 (85eff7c80 2026-01-15)`

## Command

```bash
VALKEY_ADDR=127.0.0.1:6389 cargo bench \
  --manifest-path benchmarks/compare-buffa-beta5/Cargo.toml \
  -p compare-buffa-beta5-runner \
  --bench fortunes
```

`VALKEY_ADDR` pointed at a manually started local `valkey/valkey:8-alpine` container. The benchmark runner seeded that instance and reused it for all three targets.

## Method

- Protocol: Connect
- Warmup: `3.0s`
- Measurement: `10.0s`
- Concurrency levels: `16`, `64`, `256`
- Uniform client: generated `connect-rust` `FortuneServiceClient`
- `connect-rust` server target: native `connectrpc::server::Server`
- `connectrpc-axum` targets: loopback Axum servers

## Results

| Implementation | Concurrency | req/s | p50 (us) | p99 (us) |
|---|---:|---:|---:|---:|
| buffa | 16 | 120634 | 123 | 362 |
| v0.1.0 | 16 | 121692 | 122 | 342 |
| connect-rust | 16 | 115749 | 129 | 379 |
| buffa | 64 | 187062 | 324 | 647 |
| v0.1.0 | 64 | 191728 | 319 | 614 |
| connect-rust | 64 | 175927 | 348 | 661 |
| buffa | 256 | 232850 | 1076 | 1775 |
| v0.1.0 | 256 | 233849 | 1072 | 1760 |
| connect-rust | 256 | 222244 | 1124 | 1935 |

## Read

- This aligned harness is much closer to `connect-rust`'s upstream Fortune benchmark methodology than the first local Fortune run.
- On this Connect-protocol benchmark, `buffa` and `v0.1.0` remain effectively tied.
- `connect-rust` is still behind both `connectrpc-axum` targets in this local setup, but the gap is now modest rather than pathological.
- At `c=256`, the local `connect-rust` result (`222244 req/s`) is much closer to the upstream `connect-rust` Connect-protocol Fortune figure (`245173 req/s`) than the earlier local harness was.

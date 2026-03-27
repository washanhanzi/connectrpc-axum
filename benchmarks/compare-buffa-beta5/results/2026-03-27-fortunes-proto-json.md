# Fortune Benchmark (Proto + JSON): 2026-03-27

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

`VALKEY_ADDR` pointed at a manually started local `valkey/valkey:8-alpine` container. The benchmark runner seeded that instance and reused it for all targets.

## Method

- Benchmarks: `connect-proto`, `connect-json`
- Warmup: `3.0s`
- Measurement: `10.0s`
- Concurrency levels: `16`, `64`, `256`
- Proto client: generated `connect-rust` `FortuneServiceClient`
- JSON client: `reqwest` sending Connect unary `application/json` requests
- `connect-rust` server target: native `connectrpc::server::Server`
- `connectrpc-axum` targets: loopback Axum servers

## Results

| Benchmark | Implementation | Concurrency | req/s | p50 (us) | p99 (us) |
|---|---|---:|---:|---:|---:|
| connect-proto | buffa | 16 | 125937 | 118 | 351 |
| connect-proto | v0.1.0 | 16 | 126199 | 118 | 352 |
| connect-proto | connect-rust | 16 | 115923 | 128 | 380 |
| connect-json | buffa | 16 | 105588 | 142 | 386 |
| connect-json | v0.1.0 | 16 | 106250 | 141 | 375 |
| connect-json | connect-rust | 16 | 102515 | 146 | 373 |
| connect-proto | buffa | 64 | 190453 | 321 | 618 |
| connect-proto | v0.1.0 | 64 | 190232 | 320 | 625 |
| connect-proto | connect-rust | 64 | 176769 | 346 | 664 |
| connect-json | buffa | 64 | 160110 | 380 | 712 |
| connect-json | v0.1.0 | 64 | 161600 | 377 | 702 |
| connect-json | connect-rust | 64 | 152771 | 401 | 732 |
| connect-proto | buffa | 256 | 231889 | 1083 | 1775 |
| connect-proto | v0.1.0 | 256 | 232609 | 1078 | 1771 |
| connect-proto | connect-rust | 256 | 224413 | 1116 | 1873 |
| connect-json | buffa | 256 | 190549 | 1317 | 2171 |
| connect-json | v0.1.0 | 256 | 191024 | 1312 | 2158 |
| connect-json | connect-rust | 256 | 187646 | 1334 | 2259 |

## Read

- `buffa` and `v0.1.0` remain effectively tied on both proto and JSON Fortune workloads.
- On proto, `v0.1.0` is marginally ahead at `16` and `256`, while `buffa` is marginally ahead at `64`.
- On JSON, `v0.1.0` is slightly ahead across all three tested concurrency levels.
- `connect-rust` trails both `connectrpc-axum` targets on both proto and JSON in this local setup, but the gap remains modest relative to the earlier unaligned harness.

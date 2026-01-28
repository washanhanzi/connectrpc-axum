# Oracle Review: connectrpc-axum-client

## Gemini's Perspective

Gemini provided a **positive assessment**, rating the implementation as production-ready:

- **API Design**: Praised the builder pattern, dual interceptor design (`Interceptor` + `MessageInterceptor`), and zero-cost composition via `Chain<A, B>`
- **Error Handling**: Found the unified `ClientError` hierarchy clean, with robust retry logic following gRPC backoff spec
- **Protocol Compliance**: Verified correct framing (5-byte envelope), content types, compression handling, and HTTP/2 enforcement
- **Code Organization**: Noted clean modular separation and effective use of shared core types

**Suggested improvements**: GET request support for idempotent calls, WASM support constraints noted as external

---

## Codex's Perspective

Codex provided a **more critical review**, identifying several issues:

### High Severity
- [ ] Streaming interceptors may be broken - `intercept_request` called with `()` instead of message types, and stream hooks never invoked
  - `connectrpc-axum-client/src/client.rs:622`
  - `connectrpc-axum-client/src/client.rs:841`
  - `connectrpc-axum-client/src/client.rs:1128`
  - `connectrpc-axum-client/src/config/interceptor.rs:294`

### Medium Severity
- [ ] Documentation describes `reqwest` transport but implementation uses `HyperTransport`
  - `connectrpc-axum-client/src/lib.rs:215`
  - `connectrpc-axum-client/src/lib.rs:435`
  - `connectrpc-axum-client/Cargo.toml:91`

- [ ] Header interceptors in streaming create fresh `HeaderMap` instead of operating on existing headers (inconsistent with unary)
  - `connectrpc-axum-client/src/client.rs:623`
  - `connectrpc-axum-client/src/client.rs:842`
  - `connectrpc-axum-client/src/client.rs:1129`

### Low Severity
- [ ] Questionable error codes - extra response frames return `Code::Unimplemented`, unsupported encodings become `InvalidArgument`
  - `connectrpc-axum-client/src/client.rs:927`
  - `connectrpc-axum-client/src/client.rs:414`
  - `connectrpc-axum-client/src/error.rs:54`

- [ ] Only POST is supported; GET for idempotent unary calls not exposed
  - `connectrpc-axum-client/src/client.rs:368`

---

## Points of Agreement

1. **GET support is missing** - Both note this as an optimization opportunity
2. **Code organization is clean** - Both praise the modular structure
3. **Protocol fundamentals are correct** - Framing, content types, error handling follow spec

## Notable Differences

| Aspect | Gemini | Codex |
|--------|--------|-------|
| Streaming interceptors | "Excellent dual-trait design" | "Effectively broken" |
| Overall quality | "Production-ready" | Has high-severity issues |
| Documentation | Not flagged | Mismatches with actual implementation |

---

## Action Items

### Investigate
- [ ] Verify whether `MessageInterceptor` hooks actually fire for streaming calls
- [ ] Check if documentation accurately reflects `HyperTransport` vs `reqwest`
- [ ] Review header interception consistency between unary and streaming paths

### Consider
- [ ] Add GET request support for idempotent unary calls
- [ ] Add streaming-interceptor integration tests
- [ ] Reclassify protocol/negotiation error codes

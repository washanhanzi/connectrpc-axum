Title: Review findings: streaming protocol mismatch, timeout scope, compression negotiation, GET base64

Body:
## Summary
This issue tracks code review findings that may cause protocol incompatibilities or unexpected behavior.

## Findings
1) ConnectStreamResponse always encodes as JSON and never uses request context
- Impact: server-streaming responses are sent as JSON with `application/connect+json` even for proto requests; negotiated compression is ignored.
- Evidence: `connectrpc-axum/src/message/stream.rs:28`, `connectrpc-axum/src/message/stream.rs:38`, `connectrpc-axum/src/message/stream.rs:64`, `connectrpc-axum/src/message/stream.rs:132`
- Suggested fix: plumb request context into `ConnectStreamResponse` or deprecate this type in favor of `ConnectResponse<StreamBody<...>>` which already respects context.

2) Request timeouts do not apply to streaming bodies
- Impact: handler timeouts only cover the initial future; server-streaming responses can continue indefinitely after the timeout elapses.
- Evidence: `connectrpc-axum/src/layer.rs:178`, `connectrpc-axum/src/layer.rs:190`
- Suggested fix: consider applying deadlines to streaming bodies or clarifying that timeouts only apply to handler execution.

3) Accept-Encoding negotiation ignores q-values
- Impact: responses may be gzip-compressed even when the client explicitly disables gzip (e.g., `gzip;q=0`).
- Evidence: `connectrpc-axum/src/context/compression.rs:78`
- Suggested fix: parse q-values and respect `q=0` for gzip.

4) GET base64 decoding uses padded URL-safe decoding
- Impact: valid unpadded base64url payloads can fail to decode.
- Evidence: `connectrpc-axum/src/message/request.rs:257`
- Suggested fix: accept unpadded base64url (e.g., `URL_SAFE_NO_PAD`) or tolerate missing padding.

## Tests to add
- GET base64url decoding with unpadded input.
- Accept-Encoding negotiation with `gzip;q=0`.
- Streaming timeout behavior (if the timeout is intended to apply to the body).

## Notes
`ConnectStreamResponse` is documented as having protocol set automatically, but no code path sets it today. If this type is legacy, consider deprecating it or updating docs to avoid confusing users.

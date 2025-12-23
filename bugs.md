  - Medium: Response encoding panics on serialization failures (expect), which can
    crash the server on invalid JSON/proto serialization (e.g., map keys not
    string). connectrpc-axum/src/response.rs:38, connectrpc-axum/src/
    response.rs:114, connectrpc-axum/src/stream_response.rs:73-86.
  - Medium: Streaming errors drop metadata/trailers; the EndStreamResponse body
    only includes { "error": err }, so ConnectError::meta never reaches clients in
    stream responses. connectrpc-axum/src/stream_response.rs:82-86, connectrpc-
    axum/src/error.rs:54-94.
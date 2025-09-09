Axum ecosystem compatibility for ConnectRPC, split into two focused parts.

## Part 1 — ConnectRPC Backend (Axum‑native)

- Overview: Handlers are regular Axum handlers integrated via `ConnectHandlerWrapper<F>` (alias `ConnectHandler<F>`), which implements `axum::handler::Handler`.

- Extractors rule: Any number of `FromRequestParts<S>` extractors first, and the final body extractor must be `ConnectRequest<Req>`.
  - Matches `impl Handler<(...parts..., ConnectRequest<Req>), S> for ConnectHandlerWrapper<F>` in `connectrpc-axum/src/handler.rs`.

- Handler signature: `F: Fn(…parts…, ConnectRequest<Req>) -> impl Future<Output = Result<ConnectResponse<Resp>, ConnectError>>`.
  - `Req`: `prost::Message + serde::de::DeserializeOwned + Default + Send + Sync + 'static`.
  - `Resp`: `prost::Message + serde::Serialize + Send + Clone + Sync + 'static`.

- Routing: Use `post_connect(handler)` to get a `MethodRouter<S>` and mount it on an Axum `Router`.
  - Example path suffixes: `/hello.HelloWorldService/SayHello`, `/hello.HelloWorldService/SayHelloStream`.

- Example:

```rust
use axum::{extract::{Query, State}, Router};
use connectrpc_axum::prelude::*;

async fn say_hello(
    Query(_p): Query<Pagination>,
    State(_s): State<AppState>,
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse(HelloResponse { message: format!("Hello, {}!", req.name.unwrap_or_default()) }))
}

let router = Router::new().route(
    "/hello.HelloWorldService/SayHello",
    connectrpc_axum::post_connect(say_hello),
);
```

- Request/Response behavior:
  - `ConnectRequest<T>` supports `POST` with `application/proto` or `application/json`, and `GET` via Connect query params (`connect=v1`, `encoding`, `message`, optional `base64`).
  - `ConnectResponse<T>` implements `IntoResponse` (JSON by default) and is returnable as `Result<ConnectResponse<T>, ConnectError>`.

```text
[Axum]
  +----------------------------+          implements          +---------------------------+
  |  ConnectHandlerWrapper<F>  | ---------------------------> |  axum::handler::Handler   |
  +----------------------------+                               +---------------------------+
                |
                | post_connect()
                v
        +------------------+
        |  MethodRouter<S> |
        +------------------+

```

## Part 2 — Tonic‑Compatible Server

- Overview: `TonicCompatibleHandlerWrapper<F>` implements `axum::handler::Handler` but restricts handler forms to tonic‑friendly patterns:
  - `(ConnectRequest<Req>)` with state `()`; or
  - `(axum::extract::State<S>, ConnectRequest<Req>)` with generic state `S`.

- Bridging (for generated servers):
  - `BoxedCall<Req, Resp>`: boxed callable invoked by the generated service.
  - `IntoFactory<T, Req, Resp, S>`: adapts a handler into a factory `Fn(&S) -> BoxedCall<Req, Resp>` keyed by extractor tuple `T`.
  - Helper: `unimplemented_boxed_call::<Req, Resp>()` yields `Unimplemented` if a handler is not provided.

- Generated service surface:
  - Builders: `*ServiceTonicCompatibleBuilder` (progressively adds routes and handler factories) and `*ServiceTonicCompatibleServerBuilder` (applies state, materializes handlers).
  - Service: `*TonicService` holds concrete boxed handlers and implements the Tonic server trait (`<service>_server::<Service>`) in generated code. Unary and streaming methods both call the same boxed handler; streaming is adapted to a one‑item stream for now.

- Routing helpers: Use `post_connect_tonic(handler)` to enforce tonic‑compatible signatures at the Axum layer.

- Single‑port dispatcher: `connectrpc_axum::ContentTypeSwitch<G,H>` routes gRPC requests (by `content-type: application/grpc`) to a Tonic server `G`, and all others to an Axum router `H`.

- Generated example flow:

```rust
use connectrpc_axum::prelude::*;
use hello::helloworldservice::HelloWorldServiceTonicCompatibleBuilder;

let (router, svc) = HelloWorldServiceTonicCompatibleBuilder::new()
    .say_hello(say_hello)
    .say_hello_stream(say_hello_stream)
    .with_state(app_state)
    .build();

let grpc = hello_world_service_server::HelloWorldServiceServer::new(svc);
let http = router.route("/ping", axum::routing::get(|| async { "pong" }));
let dispatch = connectrpc_axum::ContentTypeSwitch::new(grpc, http);
axum::serve(listener, tower::make::Shared::new(dispatch)).await?;
```

```text
[Axum (tonic-compatible)]
  +-------------------------------------+        implements        +---------------------------+
  |  TonicCompatibleHandlerWrapper<F>   | -----------------------> |  axum::handler::Handler   |
  +-------------------------------------+                          +---------------------------+
                |                                   (generated)
                | post_connect_tonic()                 +----------------------------------------------+
                v                                      |            <Service>TonicService               |
        +------------------+                           |   + boxed call fields (BoxedCall<Req,Resp>)   |
        |  MethodRouter<S> |                           |   + impl <service>_server::<Service>          |
        +------------------+                           +----------------------------------------------+
                                                           |
                                                           | HelloWorldServiceServer::new(svc)
                                                           v
                                          +------------------------------------+
                                          |   Tonic server (tower::Service)    |
                                          +------------------------------------+

[Single-port dispatch]
  +---------------------+        content-type: application/grpc         +-------------------+
  |  ContentTypeSwitch  | --------------------------------------------> |    gRPC (tonic)   |
  |     (dispatcher)    |                                              +-------------------+
  |                     |  otherwise                                   +-------------------+
  |                     | --------------------------------------------> |  Axum (Connect)   |
  +---------------------+                                              +-------------------+
```

Notes

- Errors map to Connect JSON responses via `ConnectError: IntoResponse`; generated tonic impls map to `tonic::Status`.
- Route paths follow `/<package>.<Service>/<Method>`; see `connectrpc-axum-examples/proto/hello.proto`.
- The dispatcher is available as `connectrpc_axum::ContentTypeSwitch` (top‑level re‑export). `TonicCompatible` remains as a type alias.

## Code Generation

Overview:

1. Base (always): We invoke `prost_build::Config` to generate all protobuf message types plus the custom service builders produced by `AxumConnectServiceGenerator` (Connect + optional tonic‑compatible builders). Every message gets:
   - `#[derive(Serialize, Deserialize)]`
   - `#[serde(rename_all = "camelCase")]`
   - `#[serde(default)]` (so omitted JSON fields, including repeated fields, deserialize to Rust defaults).
2. Tonic feature (two‑pass): When the `tonic` Cargo feature is enabled and the user calls `.with_tonic()`, we perform a second pass dedicated to gRPC server stubs without regenerating (and thus duplicating) message structs.

Two‑Pass Flow (feature = tonic):

- Pass 1 (prost + connect):
  - Configure prost with the attributes above.
  - Attach the `AxumConnectServiceGenerator` (flagged to also emit tonic‑compatible builder types).
  - If gRPC is requested, emit a `descriptor.bin` (FileDescriptorSet) to `OUT_DIR` for later reflection.
- Pass 2 (tonic server stubs only):
  - Decode `descriptor.bin` (using `prost_types::FileDescriptorSet`).
  - Recursively collect fully qualified proto type names (messages & enums, including nested) and map them to the already generated Rust types.
  - Configure `tonic_prost_build` with `build_client(false)`, `build_server(true)`, and supply `extern_path` for every collected type so tonic reuses existing structs instead of emitting new ones.
  - Generate server code into a temporary `tonic_server/` directory in `OUT_DIR`.
  - Append the server stub code to each first‑pass `<file>.rs` so a single `include!(concat!(env!("OUT_DIR"), "/<proto>.rs"));` brings in everything (messages + builders + tonic server trait impls).
  - Clean up: remove `descriptor.bin` and the temporary `tonic_server/` directory so only the merged `<proto>.rs` files remain visible to consumers.

Why extern_path:

`tonic_prost_build` ordinarily regenerates message types. By supplying `extern_path` entries (e.g. `.hello.HelloRequest` -> `crate::HelloRequest`), we direct tonic to reference the already generated prost structs, eliminating duplication and the need for brittle post‑processing (text stripping).

Nested Types:

Current mapping strategy anticipates nested messages/enums by constructing fully qualified names from the descriptor set; if/when nested declarations are introduced, the extern mapping logic can be extended (prost typically flattens or modules them—adjust as needed based on actual output naming).

Artifacts:

- Final OUT_DIR after build: only consolidated `<proto>.rs` files.
- Temporary artifacts (`descriptor.bin`, `tonic_server/`) are removed post‑append to avoid accidental `include!` of intermediate files.

Extension Points / Future Work:

- Add tests that compile a sample proto with nested types to validate extern_path mapping.
- Optionally gate cleanup behind an env var (e.g. `CONNECTAXUM_DEBUG_CODEGEN=1`) for debugging generated intermediate outputs.

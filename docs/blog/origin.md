---
title: The Origin of connectrpc-axum
description: Problems I encountered and how I solved them while building a Connect RPC framework for Axum
outline: deep
---

# The Origin of connectrpc-axum

This post walks through the problems I encountered while building a Connect protocol implementation for Axum, and how I solved them.

## The Problem in axum-connect

The first issue many users encounter is with [FromRequestParts](https://github.com/AThilenius/axum-connect/issues/23).

Looking at the [handler definition](https://github.com/AThilenius/axum-connect/blob/main/axum-connect/src/handler/handler_unary.rs):

```rust
impl<TMReq, TMRes, TInto, TFnFut, TFn, TState, $($ty,)*>
    RpcHandlerUnary<TMReq, TMRes, ($($ty,)* TMReq), TState> for TFn
where
    TMReq: Message + DeserializeOwned + Default + Send + 'static,
    TMRes: Message + Serialize + Send + 'static,
    TInto: RpcIntoResponse<TMRes>,
    TFnFut: Future<Output = TInto> + Send,
    TFn: FnOnce($($ty,)* TMReq) -> TFnFut + Clone + Send + Sync + 'static,
    TState: Send + Sync + 'static,
    $( $ty: RpcFromRequestParts<TMRes, TState> + Send, )* // [!code highlight]
{
    //...
}
```

The `ty` must implement `RpcFromRequestParts`, so to make this code work:

```rust
async fn say_hello_unary(Host(host): Host, request: HelloRequest) -> Result<HelloResponse, Error> {
    // ...
}
```

You need an [implementation like this](https://github.com/AThilenius/axum-connect/blob/main/axum-connect/src/parts.rs):

```rust
#[cfg(feature = "axum-extra")]
impl<M, S> RpcFromRequestParts<M, S> for Host // [!code highlight]
where
    M: Message,
    S: Send + Sync,
{
    type Rejection = RpcError;

    async fn rpc_from_request_parts(
        parts: &mut http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        //...
    }
}
```

And this is true for **every** user-defined extractor. My first thought was: why not use `FromRequestParts` directly? Why do we need `TMRes` in `RpcFromRequestParts<TMRes, TState>`?

Another issue: axum-connect has separate handler traits — [`RpcHandlerUnary`](https://github.com/AThilenius/axum-connect/blob/main/axum-connect/src/handler/handler_unary.rs) and [`RpcHandlerStream`](https://github.com/AThilenius/axum-connect/blob/main/axum-connect/src/handler/handler_stream.rs). To fully support streaming, you'd need four handler types: unary, server stream, client stream, and bidi stream. That's a lot of duplication.

## The gRPC Requirement

I have a service that uses the Connect protocol to communicate with the frontend, and also communicates with other backend services using bidirectional streaming gRPC.

It's nice to have a single backend service serve both Connect and gRPC traffic. I didn't want to implement gRPC from scratch. But what if I just use `tonic`?

## Goals

::: info What I Wanted
1. **Native extractors** — Instead of:
   ```rust
   async fn handler(RpcFromRequestParts<TMRes, TState>, ..., TMReq) -> TMRes
   ```
   I want:
   ```rust
   async fn handler(FromRequestParts<S>, ..., request: TMReq) -> TMRes
   ```

2. **gRPC support** — Through tonic integration

3. **Streaming support** — For client, server, and bidirectional streaming
:::

## Handler Design

To spare you all the back-and-forth I had with Claude, I landed on this handler signature:

```rust
async fn handler(
    FromRequestParts<S>,
    FromRequestParts<S>,
    ...,
    ConnectRequest<Req>
) -> Result<ConnectResponse<Resp>, ConnectError>
```

`ConnectRequest<Req>` can also be `ConnectRequest<Streaming<Req>>`, and the same applies to responses.

::: tip Key Benefits
1. Supports Axum's native `FromRequestParts` — no boilerplate wrapper implementations.
2. A single handler type for both unary and streaming. Different handler traits are not needed — the distinction is resolved through type inference with the same constraint (`ConnectHandlerWrapper<F>: Handler<T, S>`).
:::

## Adding Tonic Support

The first approach was to restrict handler types:

```rust
async fn say_hello(state: State, request: ConnectRequest<Req>) -> Result<ConnectResponse<Resp>, ConnectError>
async fn say_hello(request: ConnectRequest<Req>) -> Result<ConnectResponse<Resp>, ConnectError>
```

Since tonic server handlers don't have extractors, if users wanted to use tonic, they couldn't have extractors in their handlers.

I would transform the user's handlers into a tonic server:

```rust
#[derive(Default)]
pub struct MyGreeter {
    state: S
}

#[tonic::async_trait]
impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        // Call user-defined say_hello here
        Ok(Response::new(reply))
    }
}
```

Then a `ContentTypeSwitch` service checks if `Content-Type` starts with `application/grpc` and routes to tonic accordingly.

## Layers and Compression

The next problem was handling Connect protocol features like timeout, protocol-version, and compression.

Since we're using Tower, I prefer handling this in a Tower layer. `ConnectLayer` parses protocol headers, negotiates compression, enforces timeouts, and stores a `ConnectContext` in request extensions.

This can also be done with proper code organization — in connectrpc-axum, when users don't provide `ConnectLayer`, the handler gets a default `ConnectContext`. I use the layer approach to make Connect features configurable and composable. It's a personal preference.

### The Compression Challenge

Tower handles compression using [`async_compression`](https://github.com/tower-rs/tower-http/blob/main/tower-http/src/compression/body.rs), which is fairly complicated. I didn't want to reinvent the wheel.

Why not just use tower-http's compression layer directly? I came up with a 3-layer design:

```
BridgeLayer → Tower CompressionLayer → ConnectLayer
```

::: details How the layers work
- **BridgeLayer** — Detects streaming requests (content-type `application/connect+*`) and prevents Tower from compressing/decompressing them by setting `Accept-Encoding: identity` and removing `Content-Encoding`
- **Tower CompressionLayer** — Handles HTTP-level compression (`Content-Encoding`) for unary requests only
- **ConnectLayer** — Handles per-envelope compression (`Connect-Content-Encoding`) for streaming requests
:::

This avoids the complexity of `async_compression`. Since each streaming message is framed in an envelope, I use synchronous buffer-based compression (`flate2`, `brotli`, `zstd` crates) on each message independently.

### Enabling Extractors in Tonic

What if I added a layer to capture all the request parts (method, URI, headers) before tonic consumes the request body? That way, handlers could still use `FromRequestParts` extractors even when running through tonic.

This became `FromRequestPartsLayer`, which clones the request metadata into extensions before tonic takes ownership. Handlers can then reconstruct the parts and run extractors against them.

The result: **users can write Axum handlers with extractors, and connectrpc-axum serves them for both Connect and gRPC protocols.**

::: info Learn More
If you're interested in tonic compatibility and `MakeServiceBuilder`, the [architecture document](/guide/architecture) provides an overview.
:::

## Client

I realized that if someone wanted to build a Connect protocol client, they would reuse the core types and structs from the server implementation. So I decided to build the client in the same repo.

I chose to build the client on `hyper` instead of `reqwest`. Part of it was personal interest — I wanted to explore hyper's lower-level HTTP primitives. But there's also a practical reason: [RPC-level interceptors](https://github.com/washanhanzi/connectrpc-axum/issues/29). With `reqwest-middleware`, you only get HTTP-level middleware that sees the raw request/response once. For streaming, the body just flows through — you can't intercept individual messages. connect-go solves this with interceptors that wrap the streaming connection, allowing them to see every `Send` and `Receive` call. To implement something similar in Rust, I needed more control over the connection lifecycle than reqwest provides. Building on hyper directly gives me that flexibility.

## Wrapping Up

I hope this post gives people some ideas when building their own Connect RPC Rust framework.

Suggestions and comments are welcome — feel free to open an [issue](https://github.com/washanhanzi/connectrpc-axum).

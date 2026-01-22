why create a new repository?
===========================

# the problem in axum-connect

the first thing a lot of user hit is the FromRequestPart(https://github.com/AThilenius/axum-connect/issues/23)

looking at the handler definition(https://github.com/AThilenius/axum-connect/blob/main/axum-connect/src/handler/handler_unary.rs):

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
            $( $ty: RpcFromRequestParts<TMRes, TState> + Send, )*
        {
            //...
        }
```

the `ty` need to be RpcFromRequestParts, so in order to make this code work:

```rust
async fn say_hello_unary(Host(host): Host, request: HelloRequest) -> Result<HelloResponse, Error> {
...
}
```

we need(https://github.com/AThilenius/axum-connect/blob/main/axum-connect/src/parts.rs):
```rust
#[cfg(feature = "axum-extra")]
impl<M, S> RpcFromRequestParts<M, S> for Host
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

and this is true for every user defined extractor.

So my first thought is why not `FromRequestParts` directly? Why we need `TMRes` in `RpcFromRequestParts<TMRes, TState>`?

# i want gRPC

i have a service use connect protocol to communicate to frontend, and it also communicate with other backend service use bidirection streaming gRPC protocol.

Alec state in `axum-connect` readme(https://github.com/AThilenius/axum-connect) the gRPC is non-goal:
> To support every feature gRPC does
You get a lot of this already with Axum, but gRPC is a monster that I don't wish to reproduce. That complexity is useful for Google, and gets in the way for pretty much everyone else.

I also don't want to touch gRPC, but what if i just use `tonic`?

# the initial goals

so my initial motivation is:

- instaed of `async fn handler(RpcFromRequestParts<TMRes, TState>, RpcFromRequestParts<TMRes, TState>, ..., TMReq) -> TMRes`

i want `async fn handler(FromRequestParts<S>, FromRequestParts<S>, ..., request: TMReq) -> TMRes`

- support gRPC protocol through tonic
- try to support stream

the first 2 is the goal, the 3rd is some added feature would nice to have, but at last i implement it.

# a new handler

to save you all the tokens i have with claude. i came up with a new handler design:

`async fn hanlder(FromRequestParts<S>, FromRequestParts<S>,...,ConnectRequest<Req>)->Result<ConnectResponse<Resp>,ConnectError>`

and `ConnectRequest<Req>` can also be `ConnectRequest<Streaming<Req>>`, and same for response.

1. it support axum native FromRequestParts, you don't need write bolierplate code to impl `RpcFromRequestParts`.
2. the single handler can support both unary and streaming request/response. no need for handler_stream(https://github.com/AThilenius/axum-connect/blob/main/axum-connect/src/handler/handler_stream.rs) and handler_unary(https://github.com/AThilenius/axum-connect/blob/main/axum-connect/src/handler/handler_unary.rs)

at this time, it's almost a complete rewrite of axum-connect, because i changed the core handler type, so i just create a new repository.

# tonic support

the first idea is a user can only provide a restricted handler types:

```rust
async fn say_hello(state: State, request: ConnectRequest<Req>) -> Result<ConnectResponse<Resp>, ConnectError>
async fn say_hello(request: ConnectRequest<Req>) -> Result<Resp, ConnectError>
```

since the tonic server handler don't have extractors, if the user want to use tonic, they can't have extractors in the handler.

i will transform user's handlers to tonic server, something like:

```rust
#[derive(Default)]
pub struct MyGreeter {
    State: S
}

#[tonic::async_trait]
impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        //call user defined say_hello here
        Ok(Response::new(reply))
    }
}
```

this is the basic idea (later i find a way to let handlers have extractors). and I just need a tower layer to detect request header and route grpc traffic to tonic.

# layers and compression

the next problem is how to deal with connect protocol features, like timeout, protocol-version, compression.

I think it's possible to orgnize the code to have a single function or struct to deal with there features, and use it in every handler. (this is happened when user use connectrpc-axum without ConnectLayer, if the handler fail to extract the request extension, it will use a default one)

but since we are using tower, i like it to be done in a tower layer, so I create a ConnectLayer to deal with all the connect protocol features(mostly header validation and parsing), and inject the information into request extensions for handler to use, this make the connect protocol feature configurable and composable.

another problem is compression, tower did this, and it use `
async_compression` and it's fairly complicated(https://github.com/tower-rs/tower-http/blob/main/tower-http/src/compression/body.rs), i don't want to reinvent the wheel.

And again, since we are already using tower, why not just use tower-http compression layer directly? i came up with a 3 layer design: `bridge layer` -> `tower compression layer` -> `connect layer`.

the bridge layer will detect it's unary or streaming request, if it's unary, it will let tower compression layer to do the work, and if it's streaming, the compression and decompression will be done in the connect layer.

And this save me from dealing with async_compression, i just compress and decompress with buffer since i'm only deal with every streaming message evelope, not compress and decompress on the whole request and response body.

So at this time, another idea just came to me, what if i add a layer to extract all the FromRequestParts information, so user can use extractors in tonic handlers?

so it finally became, user can write axum handler with extractors, and connectrpc-axum will servce it for both connect and grpc protocol.

A lot of details haven't been covered here, if you are interested on the tonic compatibility and MakeServiceBuilder, [architecture document](https://washanhanzi.github.io/connectrpc-axum/guide/architecture.html) will give you some first idea before jump into the code.

# client

at first I don't want a client implementation since i'm not using one, but later I find in order to create a connect protocol client, i would reuse the types or core structs already in my server implementation, so i think i can do that together.

# final word

Really appreciate Alec's work on axum-connect, without it, I won't have the idea of building this project. and i hope this post will give people some ideas when they are building their own connect rpc rust framework. Any suggestion or comment is welcome, feel free to open an [issue](https://github.com/washanhanzi/connectrpc-axum).





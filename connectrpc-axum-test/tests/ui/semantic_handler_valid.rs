use axum::handler::Handler;
use connectrpc_axum::{handler::ConnectHandlerWrapper, prelude::*};
use futures::stream;
use prost::Message;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct HelloRequest {
    #[prost(string, tag = "1")]
    pub name: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct HelloResponse {
    #[prost(string, tag = "1")]
    pub message: String,
}

type ResponseStream = stream::Empty<Result<HelloResponse, ConnectError>>;

struct HelloWorldServiceBuilder<S = ()> {
    _state: PhantomData<S>,
}

impl<S> HelloWorldServiceBuilder<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn new() -> Self {
        Self {
            _state: PhantomData,
        }
    }

    fn say_hello_stream<F, T>(self, _handler: F) -> Self
    where
        ConnectHandlerWrapper<F, HelloRequest, HelloResponse>: Handler<T, S>,
        F: Clone + Send + Sync + 'static,
        T: 'static,
    {
        self
    }
}

async fn say_hello_stream(
    ConnectRequest(_req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<StreamBody<ResponseStream>>, ConnectError> {
    Ok(ConnectResponse::new(StreamBody::new(stream::empty())))
}

fn main() {
    let _ = HelloWorldServiceBuilder::new().say_hello_stream(say_hello_stream);
}

use connectrpc_axum::prelude::*;
use crate::{HelloRequest, HelloResponse, hello_world_service_connect};
use futures::Stream;

async fn say_hello_stream(
    ConnectRequest(_req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    // Return an error immediately without sending any messages.
    // We need a concrete stream type for the Ok variant; use async_stream to
    // provide one even though this branch is never reached.
    if true {
        return Err(ConnectError::new(Code::Internal, "something went wrong"));
    }

    let stream = async_stream::stream! {
        yield Ok(HelloResponse {
            message: String::new(),
            response_type: None,
        });
    };
    Ok(ConnectResponse::new(StreamBody::new(stream)))
}

pub async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    let router = hello_world_service_connect::HelloWorldServiceBuilder::new()
        .say_hello_stream(say_hello_stream)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .build();

    axum::serve(listener, app).await?;
    Ok(())
}

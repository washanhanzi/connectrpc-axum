use connectrpc_axum::prelude::*;
use crate::{HelloRequest, HelloResponse, hello_world_service_connect};
use futures::Stream;

async fn say_hello_stream(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    let name = req.name.unwrap_or_else(|| "World".to_string());

    let stream = async_stream::stream! {
        yield Ok(HelloResponse { message: format!("Hello, {}!", name), response_type: None });
        yield Ok(HelloResponse { message: format!("How are you, {}?", name), response_type: None });
        yield Ok(HelloResponse { message: format!("Goodbye, {}!", name), response_type: None });
    };

    Ok(ConnectResponse::new(StreamBody::new(stream)))
}

pub async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    let (connect_router, grpc_server) =
        hello_world_service_connect::HelloWorldServiceTonicCompatibleBuilder::new()
            .say_hello_stream(say_hello_stream)
            .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(connect_router)
        .add_grpc_service(grpc_server)
        .build();

    axum::serve(listener, tower::make::Shared::new(app)).await?;
    Ok(())
}

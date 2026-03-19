use crate::{HelloRequest, HelloResponse, hello_world_service_connect};
use connectrpc_axum::prelude::*;
use futures::Stream;

async fn say_hello_stream(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    let name = req.name.unwrap_or_else(|| "World".to_string());

    let stream = async_stream::stream! {
        match name.as_str() {
            "Small" => {
                yield Ok(HelloResponse {
                    message: "Hi".to_string(),
                    response_type: None,
                ..Default::default()});
                yield Ok(HelloResponse {
                    message: "Hey".to_string(),
                    response_type: None,
                ..Default::default()});
            }
            "Large" => {
                yield Ok(HelloResponse {
                    message: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string(),
                    response_type: None,
                ..Default::default()});
            }
            _ => {
                yield Ok(HelloResponse {
                    message: format!("Hello, {}!", name),
                    response_type: None,
                ..Default::default()});
            }
        }
    };

    Ok(ConnectResponse::new(StreamBody::new(stream)))
}

pub async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    let router = hello_world_service_connect::HelloWorldServiceBuilder::new()
        .say_hello_stream(say_hello_stream)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .send_max_bytes(64)
        .build();

    axum::serve(listener, app).await?;
    Ok(())
}

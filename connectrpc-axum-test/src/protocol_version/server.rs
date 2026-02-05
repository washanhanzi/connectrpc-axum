use connectrpc_axum::prelude::*;
use crate::{HelloRequest, HelloResponse, hello_world_service_connect};

async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let name = req.name.unwrap_or_else(|| "World".to_string());
    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello, {}!", name),
        response_type: None,
    }))
}

pub async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    let router = hello_world_service_connect::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .require_protocol_header(true)
        .build();

    axum::serve(listener, app).await?;
    Ok(())
}

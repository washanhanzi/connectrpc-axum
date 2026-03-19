use crate::{HelloRequest, HelloResponse, hello_world_service_connect};
use connectrpc_axum::prelude::*;
use std::time::Duration;

async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    tokio::time::sleep(Duration::from_millis(500)).await;

    let name = req.name.unwrap_or_else(|| "World".to_string());
    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello, {}! (after 500ms delay)", name),
        response_type: None,
        ..Default::default()
    }))
}

pub async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    let router = hello_world_service_connect::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .build();

    axum::serve(listener, app).await?;
    Ok(())
}

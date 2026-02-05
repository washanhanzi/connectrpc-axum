use connectrpc_axum::prelude::*;
use crate::{HelloRequest, HelloResponse, hello_world_service_connect};

async fn say_hello(
    ConnectRequest(_req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Err(
        ConnectError::new(Code::InvalidArgument, "name is required")
            .with_meta("x-custom-meta", "custom-value")
            .with_meta("x-request-id", "test-123"),
    )
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

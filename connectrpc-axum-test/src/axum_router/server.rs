use axum::{Router, routing::get};
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

async fn health() -> &'static str {
    "ok"
}

async fn metrics() -> &'static str {
    "requests_total 42\nrequests_errors 0"
}

pub async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    let router = hello_world_service_connect::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .build();

    let axum_router = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics));

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .add_axum_router(axum_router)
        .build();

    axum::serve(listener, app).await?;
    Ok(())
}

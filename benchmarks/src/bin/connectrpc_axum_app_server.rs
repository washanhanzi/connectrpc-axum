use std::sync::Arc;

use anyhow::Context;
use axum::extract::State;
use axum::serve::ListenerExt;
use connectrpc_axum::CompressionConfig;
use connectrpc_axum::prelude::*;
use connectrpc_axum_benchmarks::support::{ValkeyPool, load_response};
use connectrpc_axum_benchmarks::{
    GetFortunesRequest, GetFortunesResponse, fortune_service_connect,
};
use tonic::codec::CompressionEncoding;

const VALKEY_POOL_SIZE: usize = 8;

async fn get_fortunes(
    State(pool): State<Arc<ValkeyPool>>,
    ConnectRequest(_request): ConnectRequest<GetFortunesRequest>,
) -> Result<ConnectResponse<GetFortunesResponse>, ConnectError> {
    let response = load_response(pool.as_ref())
        .await
        .map_err(|err| ConnectError::new_internal(err.to_string()))?;
    Ok(ConnectResponse::new(response))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let valkey_addr = std::env::args()
        .nth(1)
        .context("usage: connectrpc_axum_app_server <valkey_addr>")?;
    let pool = Arc::new(ValkeyPool::connect(&valkey_addr, VALKEY_POOL_SIZE).await?);

    let (connect_router, grpc_server) =
        fortune_service_connect::FortuneServiceTonicCompatibleBuilder::new()
            .get_fortunes(get_fortunes)
            .with_state(pool)
            .build();

    let service = connectrpc_axum::MakeServiceBuilder::new()
        .compression(CompressionConfig::new(1))
        .add_router(connect_router)
        .add_grpc_service_with(grpc_server, |svc| {
            svc.accept_compressed(CompressionEncoding::Gzip)
                .send_compressed(CompressionEncoding::Gzip)
        })
        .build();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    println!("{addr}");
    let listener = listener.tap_io(|tcp_stream| {
        let _ = tcp_stream.set_nodelay(true);
    });

    tokio::select! {
        result = axum::serve(listener, tower::make::Shared::new(service)) => result?,
        _ = tokio::signal::ctrl_c() => {}
    }

    Ok(())
}

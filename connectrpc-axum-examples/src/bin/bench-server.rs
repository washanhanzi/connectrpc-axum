use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{BenchRequest, BenchResponse, bench_service_connect};

async fn unary(
    ConnectRequest(req): ConnectRequest<BenchRequest>,
) -> Result<ConnectResponse<BenchResponse>, ConnectError> {
    Ok(ConnectResponse::new(BenchResponse {
        payload: req.payload,
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (connect_router, grpc_server) =
        bench_service_connect::BenchServiceTonicCompatibleBuilder::new()
            .unary(unary)
            .build();

    let service = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(connect_router)
        .add_grpc_service(grpc_server)
        .build();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    println!("{addr}");

    tokio::select! {
        result = axum::serve(listener, tower::make::Shared::new(service)) => result?,
        _ = tokio::signal::ctrl_c() => {}
    }

    Ok(())
}

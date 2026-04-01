use axum::serve::ListenerExt;
use connectrpc_axum::CompressionConfig;
use connectrpc_axum::prelude::*;
use connectrpc_axum_benchmarks::{BenchRequest, BenchResponse, bench_service_connect};
use tonic::codec::CompressionEncoding;

async fn unary(
    ConnectRequest(request): ConnectRequest<BenchRequest>,
) -> Result<ConnectResponse<BenchResponse>, ConnectError> {
    Ok(ConnectResponse::new(BenchResponse {
        payload: request.payload,
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (connect_router, grpc_server) =
        bench_service_connect::BenchServiceTonicCompatibleBuilder::new()
            .unary(unary)
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

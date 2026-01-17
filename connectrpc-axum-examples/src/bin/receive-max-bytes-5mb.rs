//! Integration test server for receive_max_bytes with 5MB limit.
//!
//! Run with: cargo run --bin receive-max-bytes-5mb
//! Test with Go client: go test -v -run TestReceiveMaxBytes5MB

use connectrpc_axum::MakeServiceBuilder;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{EchoRequest, EchoResponse, echoservice};
use futures::StreamExt;
use std::net::SocketAddr;

async fn echo(
    ConnectRequest(req): ConnectRequest<EchoRequest>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    Ok(ConnectResponse::new(EchoResponse {
        message: format!("Echo: {} bytes received", req.message.len()),
    }))
}

async fn echo_client_stream(
    ConnectRequest(streaming): ConnectRequest<Streaming<EchoRequest>>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    let mut stream = streaming.into_stream();
    let mut total_bytes = 0;
    let mut msg_count = 0;

    while let Some(result) = stream.next().await {
        match result {
            Ok(msg) => {
                total_bytes += msg.message.len();
                msg_count += 1;
            }
            Err(e) => return Err(e),
        }
    }

    Ok(ConnectResponse::new(EchoResponse {
        message: format!(
            "Received {} messages, {} total bytes",
            msg_count, total_bytes
        ),
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let router = echoservice::EchoServiceBuilder::new()
        .echo(echo)
        .echo_client_stream(echo_client_stream)
        .build();

    // Configure with 5MB receive_max_bytes limit
    let service = MakeServiceBuilder::new()
        .add_router(router)
        .receive_max_bytes(5 * 1024 * 1024) // 5MB limit
        .build();

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== receive_max_bytes 5MB Test Server ===");
    println!("Server listening on http://{}", addr);
    println!("Configuration: receive_max_bytes = 5MB");

    axum::serve(listener, service).await?;
    Ok(())
}

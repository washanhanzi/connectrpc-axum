use connectrpc_axum::CompressionConfig;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{EchoRequest, EchoResponse, echoservice};
use futures::StreamExt;
// SocketAddr now provided by server_addr()

async fn echo_client_stream(
    ConnectRequest(streaming): ConnectRequest<Streaming<EchoRequest>>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    let mut messages = Vec::new();
    let mut stream = streaming.into_stream();

    while let Some(result) = stream.next().await {
        match result {
            Ok(req) => {
                messages.push(req.message);
            }
            Err(e) => {
                return Err(e);
            }
        }
    }

    Ok(ConnectResponse::new(EchoResponse {
        message: format!(
            "Received {} messages: [{}]",
            messages.len(),
            messages.join(", ")
        ),
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let router = echoservice::EchoServiceBuilder::new()
        .echo_client_stream(echo_client_stream)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .compression(CompressionConfig::new(100))
        .add_router(router)
        .build();

    let addr = connectrpc_axum_examples::server_addr();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("=== Client Streaming Compression (All Algorithms) Test Server ===");
    println!("Server listening on http://{}", addr);
    println!();
    println!("Supported encodings: gzip, deflate, br, zstd");
    println!("Service: EchoService");
    println!("  - EchoClientStream: POST /echo.EchoService/EchoClientStream");
    println!();
    println!("Test with:");
    println!("  go test -v -run TestClientStreamingDecompression_Deflate");
    println!("  go test -v -run TestClientStreamingDecompression_Brotli");
    println!("  go test -v -run TestClientStreamingDecompression_Zstd");

    axum::serve(listener, app).await?;
    Ok(())
}

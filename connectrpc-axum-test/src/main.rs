mod pb {
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
}
use pb::*;

mod echo_pb {
    include!(concat!(env!("OUT_DIR"), "/echo.rs"));
}
use echo_pb::*;

pub mod socket;
mod server_timeout;
mod connect_unary;
mod connect_server_stream;
mod error_details;
mod protocol_version;
mod streaming_error;
mod send_max_bytes;
mod receive_max_bytes;
mod get_request;
mod unary_error_metadata;
mod endstream_metadata;
mod extractor_connect_error;
mod extractor_http_response;
mod protocol_negotiation;
mod axum_router;
mod streaming_send_max_bytes;
mod streaming_receive_max_bytes;
mod streaming_extractor;
mod receive_max_bytes_5mb;
mod receive_max_bytes_unlimited;
mod connect_client_stream;
mod connect_bidi_stream;
mod streaming_compression_gzip;
mod client_streaming_compression;
mod compression_algos;
mod streaming_extractor_client;
mod tonic_unary;
mod tonic_server_stream;
mod tonic_bidi_server;
mod grpc_web;
mod tonic_extractor;
mod idempotency_get_connect_client;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pid = std::process::id();
    let rust_sock = socket::TestSocket::new(&format!("connectrpc-test-{pid}-rust"))?;
    let go_sock = socket::TestSocket::new(&format!("connectrpc-test-{pid}-go"))?;

    server_timeout::run(&rust_sock, &go_sock).await?;
    connect_unary::run(&rust_sock, &go_sock).await?;
    connect_server_stream::run(&rust_sock, &go_sock).await?;
    error_details::run(&rust_sock, &go_sock).await?;
    protocol_version::run(&rust_sock, &go_sock).await?;
    streaming_error::run(&rust_sock, &go_sock).await?;
    send_max_bytes::run(&rust_sock, &go_sock).await?;
    receive_max_bytes::run(&rust_sock, &go_sock).await?;
    get_request::run(&rust_sock, &go_sock).await?;
    unary_error_metadata::run(&rust_sock, &go_sock).await?;
    endstream_metadata::run(&rust_sock, &go_sock).await?;
    extractor_connect_error::run(&rust_sock, &go_sock).await?;
    extractor_http_response::run(&rust_sock, &go_sock).await?;
    protocol_negotiation::run(&rust_sock, &go_sock).await?;
    axum_router::run(&rust_sock, &go_sock).await?;
    streaming_send_max_bytes::run(&rust_sock, &go_sock).await?;
    streaming_receive_max_bytes::run(&rust_sock, &go_sock).await?;
    streaming_extractor::run(&rust_sock, &go_sock).await?;
    receive_max_bytes_5mb::run(&rust_sock, &go_sock).await?;
    receive_max_bytes_unlimited::run(&rust_sock, &go_sock).await?;
    connect_client_stream::run(&rust_sock, &go_sock).await?;
    connect_bidi_stream::run(&rust_sock, &go_sock).await?;
    streaming_compression_gzip::run(&rust_sock, &go_sock).await?;
    client_streaming_compression::run(&rust_sock, &go_sock).await?;
    compression_algos::run(&rust_sock, &go_sock).await?;
    streaming_extractor_client::run(&rust_sock, &go_sock).await?;
    tonic_unary::run(&rust_sock, &go_sock).await?;
    tonic_server_stream::run(&rust_sock, &go_sock).await?;
    tonic_bidi_server::run(&rust_sock, &go_sock).await?;
    grpc_web::run(&rust_sock, &go_sock).await?;
    tonic_extractor::run(&rust_sock, &go_sock).await?;
    idempotency_get_connect_client::run(&rust_sock, &go_sock).await
}

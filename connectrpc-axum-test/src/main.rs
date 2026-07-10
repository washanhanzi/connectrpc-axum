mod pb {
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
}
use pb::*;

mod echo_pb {
    include!(concat!(env!("OUT_DIR"), "/echo.rs"));
}
use echo_pb::*;

mod wkt_pb {
    include!(concat!(env!("OUT_DIR"), "/wkt.rs"));
}

mod generated {
    include!(concat!(env!("OUT_DIR"), "/protos.rs"));
}

mod axum_router;
mod client_streaming_compression;
mod compression_algos;
mod connect_bidi_stream;
mod connect_client_stream;
mod connect_server_stream;
mod connect_unary;
mod endstream_metadata;
mod error_details;
mod extractor_connect_error;
mod extractor_http_response;
mod get_request;
mod grpc_web;
mod idempotency_get_connect_client;
mod protocol_negotiation;
mod protocol_version;
mod receive_max_bytes;
mod receive_max_bytes_5mb;
mod receive_max_bytes_unlimited;
mod send_max_bytes;
mod server_timeout;
pub mod socket;
mod streaming_compression_gzip;
mod streaming_error;
mod streaming_extractor;
mod streaming_extractor_client;
mod streaming_receive_max_bytes;
mod streaming_send_max_bytes;
mod tonic_bidi_server;
mod tonic_extractor;
mod tonic_server_stream;
mod tonic_unary;
mod unary_error_metadata;

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

#[cfg(test)]
mod well_known_types_json_tests {
    use super::wkt_pb::TimestampMessage;

    #[test]
    fn generated_well_known_types_use_protobuf_json_mapping() {
        let timestamp = pbjson_types::Timestamp {
            seconds: 0,
            nanos: 0,
        };
        assert_eq!(
            serde_json::to_string(&timestamp).unwrap(),
            "\"1970-01-01T00:00:00+00:00\""
        );
        assert_eq!(
            serde_json::to_string(&TimestampMessage {
                value: Some(timestamp),
            })
            .unwrap(),
            "{\"value\":\"1970-01-01T00:00:00+00:00\"}"
        );
        assert_eq!(
            serde_json::to_string(&pbjson_types::Empty {}).unwrap(),
            "{}"
        );
    }
}

#[cfg(test)]
mod package_less_proto_tests {
    use super::generated::{PackageLessRequest, PackageLessResponse};
    use connectrpc_axum::{ConnectRequest, ConnectResponse};

    async fn call(
        ConnectRequest(request): ConnectRequest<PackageLessRequest>,
    ) -> Result<ConnectResponse<PackageLessResponse>, connectrpc_axum::ConnectError> {
        Ok(ConnectResponse::new(PackageLessResponse {
            greeting: format!("Hello, {}", request.display_name),
        }))
    }

    #[test]
    fn generated_package_less_messages_support_json_handlers() {
        let request = PackageLessRequest {
            display_name: "Ada".to_string(),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(json, r#"{"displayName":"Ada"}"#);
        assert_eq!(
            serde_json::from_str::<PackageLessRequest>(&json).unwrap(),
            request
        );

        let _router =
            super::generated::package_less_service_connect::PackageLessServiceBuilder::new()
                .call(call)
                .build();
    }
}

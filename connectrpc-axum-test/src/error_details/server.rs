use connectrpc_axum::prelude::*;
use crate::{HelloRequest, HelloResponse, hello_world_service_connect};
async fn say_hello(
    ConnectRequest(_req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    // Always return an error with details for this test server.
    // Encode a google.protobuf.StringValue as the error detail.
    let string_value = encode_string_value("provide a name");

    Err(
        ConnectError::new(Code::InvalidArgument, "name is required")
            .add_detail("google.protobuf.StringValue", string_value),
    )
}

/// Encode a google.protobuf.StringValue protobuf message manually.
///
/// StringValue has a single field:
///   string value = 1;
fn encode_string_value(s: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    // field 1, wire type 2 (length-delimited)
    prost::encoding::string::encode(1, &s.to_string(), &mut bytes);
    bytes
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

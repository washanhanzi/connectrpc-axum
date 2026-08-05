use crate::{HelloRequest, HelloResponse, hello_world_service_connect};
use axum::body::{Body, Bytes};
use axum::response::Response;
use bytes::BytesMut;
use connectrpc_axum::prelude::*;
use futures::{Stream, StreamExt};

/// Chunk size used by the re-chunking middleware. Small enough that every
/// envelope spans multiple body chunks.
const RECHUNK_SIZE: usize = 8;

/// First streamed message, crafted so re-chunking produces a decoy EndStream
/// header. The serialized body is:
///
/// ```text
/// offset 0..5   envelope header [flags=0x00][u32 length]
/// offset 5..7   proto field header [tag=0x0A][string length]
/// offset 7      'A'
/// offset 8..13  \x02 \x00 \x00 \x00 \x00
/// ```
///
/// With 8-byte chunks the second chunk starts at offset 8, exactly at the
/// planted bytes, which mimic an empty EndStream envelope header (flags with
/// the END_STREAM bit set, length 0).
pub fn decoy_message() -> String {
    "A\u{2}\0\0\0\0rechunk-decoy".to_string()
}

async fn say_hello_stream(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<
    ConnectResponse<StreamBody<impl Stream<Item = Result<HelloResponse, ConnectError>>>>,
    ConnectError,
> {
    let name = req.name.unwrap_or_else(|| "World".to_string());
    let response_stream = async_stream::stream! {
        yield Ok(HelloResponse { message: decoy_message(), response_type: None });
        yield Ok(HelloResponse { message: format!("second for {name}"), response_type: None });
        yield Ok(HelloResponse { message: format!("third for {name}"), response_type: None });
    };
    Ok(ConnectResponse::new(StreamBody::new(response_stream)))
}

/// Re-slices the response body into fixed-size chunks, so envelope frames no
/// longer align with body chunks downstream.
async fn rechunk_response(resp: Response) -> Response {
    let (parts, body) = resp.into_parts();
    let mut frames = body.into_data_stream();
    let rechunked = async_stream::stream! {
        let mut buf = BytesMut::new();
        while let Some(chunk) = frames.next().await {
            match chunk {
                Ok(bytes) => {
                    buf.extend_from_slice(&bytes);
                    while buf.len() >= RECHUNK_SIZE {
                        yield Ok::<Bytes, axum::Error>(buf.split_to(RECHUNK_SIZE).freeze());
                    }
                }
                Err(err) => {
                    yield Err(err);
                    return;
                }
            }
        }
        if !buf.is_empty() {
            yield Ok(buf.freeze());
        }
    };
    Response::from_parts(parts, Body::from_stream(rechunked))
}

pub async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    let hello_router = hello_world_service_connect::HelloWorldServiceBuilder::new()
        .say_hello_stream(say_hello_stream)
        .build()
        // Applied to the router before MakeServiceBuilder wraps it in
        // ConnectLayer, so response bodies are re-chunked between the handler
        // and ConnectLayer's deadline body wrapper.
        .layer(axum::middleware::map_response(rechunk_response));

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(hello_router)
        .build();

    axum::serve(listener, app).await?;
    Ok(())
}

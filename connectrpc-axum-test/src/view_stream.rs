use buffa::Message;
use bytes::Bytes;
use connectrpc_axum::prelude::*;
use futures::StreamExt;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::{EchoRequest, EchoResponse, echo_service_connect, socket::TestSocket};

async fn echo_client_stream(
    req: ViewStreamRequest<EchoRequest>,
) -> Result<ConnectResponse<EchoResponse>, ConnectError> {
    let mut stream = req.0.into_stream();
    let mut messages = Vec::new();

    while let Some(result) = stream.next().await {
        let msg = result?;
        messages.push(msg.message.to_string());
    }

    Ok(ConnectResponse::new(EchoResponse {
        message: format!(
            "Received {} messages: [{}]",
            messages.len(),
            messages.join(", ")
        ),
        ..Default::default()
    }))
}

async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    let router = echo_service_connect::EchoServiceBuilder::new()
        .echo_client_stream(echo_client_stream)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .build();

    axum::serve(listener, app).await?;
    Ok(())
}

pub async fn run(rust_sock: &TestSocket, _go_sock: &TestSocket) -> anyhow::Result<()> {
    let rust_listener = rust_sock.bind()?;
    let rust_server = tokio::spawn(start(rust_listener));

    rust_sock.wait_ready().await?;

    let result = run_proto_request(rust_sock).await;

    rust_server.abort();

    result
}

fn envelope_frame(flags: u8, payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u32;
    let mut buf = Vec::with_capacity(5 + payload.len());
    buf.push(flags);
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(payload);
    buf
}

async fn run_proto_request(sock: &TestSocket) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let mut body = Vec::new();
    body.extend_from_slice(&envelope_frame(
        0x00,
        &EchoRequest {
            message: "Hello".to_string(),
            ..Default::default()
        }
        .encode_to_vec(),
    ));
    body.extend_from_slice(&envelope_frame(
        0x00,
        &EchoRequest {
            message: "View".to_string(),
            ..Default::default()
        }
        .encode_to_vec(),
    ));
    body.extend_from_slice(&envelope_frame(
        0x00,
        &EchoRequest {
            message: "Stream".to_string(),
            ..Default::default()
        }
        .encode_to_vec(),
    ));
    body.extend_from_slice(&envelope_frame(0x02, b"{}"));

    let req = Request::builder()
        .method("POST")
        .uri("/echo.EchoService/EchoClientStream")
        .header("Content-Type", "application/connect+proto")
        .header("Connect-Protocol-Version", "1")
        .header("Host", "localhost")
        .body(Full::new(Bytes::from(body)))?;

    let resp = sender.send_request(req).await?;
    let status = resp.status();
    if status != 200 {
        let body = resp.into_body().collect().await?.to_bytes();
        anyhow::bail!(
            "expected HTTP 200, got {}: {}",
            status,
            String::from_utf8_lossy(&body)
        );
    }

    let body_bytes = resp.into_body().collect().await?.to_bytes();
    let response = if body_bytes.len() >= 5 && (body_bytes[0] == 0x00 || body_bytes[0] == 0x01) {
        let len = u32::from_be_bytes([body_bytes[1], body_bytes[2], body_bytes[3], body_bytes[4]])
            as usize;
        let payload = &body_bytes[5..5 + len];
        EchoResponse::decode_from_slice(payload)?
    } else {
        EchoResponse::decode_from_slice(&body_bytes)?
    };

    if response.message != "Received 3 messages: [Hello, View, Stream]" {
        anyhow::bail!("unexpected response: {:?}", response.message);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn view_stream_request_accepts_proto_client_stream() {
        let socket = TestSocket::new(&format!("connectrpc-view-stream-{}", std::process::id()))
            .expect("socket");
        let listener = socket.bind().expect("bind");
        let server = tokio::spawn(start(listener));

        socket.wait_ready().await.expect("server ready");
        run_proto_request(&socket)
            .await
            .expect("proto client stream request succeeds");

        server.abort();
    }
}

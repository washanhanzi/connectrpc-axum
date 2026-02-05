use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::socket::TestSocket;

pub struct CaseResult {
    pub name: &'static str,
    pub error: Option<String>,
}

pub async fn run_grpc_web_tests(sock: &TestSocket) -> Vec<CaseResult> {
    let err = test_grpc_web_unary(sock).await.err().map(|e| e.to_string());
    vec![CaseResult { name: "gRPC-Web unary request is accepted", error: err }]
}

async fn test_grpc_web_unary(sock: &TestSocket) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("connection error: {e}");
        }
    });

    // Build protobuf HelloRequest with name="Alice"
    // Field 1 (name), wire type 2 (length-delimited): tag = (1 << 3) | 2 = 0x0a
    let mut proto_bytes = Vec::new();
    let name = b"Alice";
    proto_bytes.push(0x0a);
    proto_bytes.push(name.len() as u8);
    proto_bytes.extend_from_slice(name);

    // gRPC frame: [compressed:1][length:4][message]
    let mut grpc_frame = Vec::new();
    grpc_frame.push(0x00); // not compressed
    grpc_frame.extend_from_slice(&(proto_bytes.len() as u32).to_be_bytes());
    grpc_frame.extend_from_slice(&proto_bytes);

    let req = Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHello")
        .header("Content-Type", "application/grpc-web+proto")
        .header("Host", "localhost")
        .body(Full::new(Bytes::from(grpc_frame)))?;

    let resp = sender.send_request(req).await?;
    let status = resp.status();

    if status != 200 {
        let body = resp.into_body().collect().await?.to_bytes();
        anyhow::bail!("expected HTTP 200, got {status}: {}", String::from_utf8_lossy(&body));
    }

    let body = resp.into_body().collect().await?.to_bytes();

    // Parse gRPC-Web response frame (same framing as gRPC)
    if body.len() < 5 {
        anyhow::bail!("gRPC-Web response too short: {} bytes", body.len());
    }

    let msg_len = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;
    if body.len() < 5 + msg_len {
        anyhow::bail!("incomplete gRPC-Web response: expected {} bytes, got {}", 5 + msg_len, body.len());
    }

    let msg_bytes = &body[5..5 + msg_len];

    // Parse protobuf HelloResponse - field 1 is message (string)
    // Look for tag 0x0a (field 1, wire type 2 = length-delimited)
    let mut i = 0;
    let mut message = String::new();
    while i < msg_bytes.len() {
        let tag = msg_bytes[i];
        i += 1;
        if tag == 0x0a {
            let len = msg_bytes[i] as usize;
            i += 1;
            message = String::from_utf8_lossy(&msg_bytes[i..i + len]).to_string();
            break;
        }
    }

    if !message.contains("Alice") {
        anyhow::bail!("expected message to contain 'Alice', got: {message:?}");
    }
    Ok(())
}

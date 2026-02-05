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

fn envelope_frame(payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u32;
    let mut buf = Vec::with_capacity(5 + payload.len());
    buf.push(0x00);
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(payload);
    buf
}

pub async fn run_tonic_server_stream_tests(sock: &TestSocket) -> Vec<CaseResult> {
    let mut results = Vec::new();

    let err = test_connect_stream(sock).await.err().map(|e| e.to_string());
    results.push(CaseResult { name: "tonic server stream via Connect protocol", error: err });

    let err = test_grpc_stream(sock).await.err().map(|e| e.to_string());
    results.push(CaseResult { name: "tonic server stream via gRPC protocol", error: err });

    results
}

async fn test_connect_stream(sock: &TestSocket) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("connection error: {e}");
        }
    });

    let enveloped = envelope_frame(br#"{"name":"Alice"}"#);

    let req = Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHelloStream")
        .header("Content-Type", "application/connect+json")
        .header("Connect-Protocol-Version", "1")
        .header("Host", "localhost")
        .body(Full::new(Bytes::from(enveloped)))?;

    let resp = sender.send_request(req).await?;
    let body_bytes = resp.into_body().collect().await?.to_bytes();

    let mut cursor = &body_bytes[..];
    let mut messages = Vec::new();
    while cursor.len() >= 5 {
        let flags = cursor[0];
        let len = u32::from_be_bytes([cursor[1], cursor[2], cursor[3], cursor[4]]) as usize;
        cursor = &cursor[5..];
        if cursor.len() < len { break; }
        let payload = &cursor[..len];
        cursor = &cursor[len..];
        if flags & 0x02 != 0 { break; }
        let json: serde_json::Value = serde_json::from_slice(payload)?;
        messages.push(json);
    }

    if messages.len() < 2 {
        anyhow::bail!("expected at least 2 messages, got {}", messages.len());
    }

    let first = messages[0].get("message").and_then(|v| v.as_str()).unwrap_or("");
    if !first.contains("Alice") {
        anyhow::bail!("expected first message to contain 'Alice', got: {first:?}");
    }
    Ok(())
}

async fn test_grpc_stream(sock: &TestSocket) -> anyhow::Result<()> {
    let (mut sender, _handle) = crate::socket::http2_connect(sock).await?;

    // Build protobuf HelloRequest with name="Bob"
    let mut proto_bytes = Vec::new();
    let name = b"Bob";
    proto_bytes.push(0x0a);
    proto_bytes.push(name.len() as u8);
    proto_bytes.extend_from_slice(name);

    // gRPC frame
    let mut grpc_frame = Vec::new();
    grpc_frame.push(0x00);
    grpc_frame.extend_from_slice(&(proto_bytes.len() as u32).to_be_bytes());
    grpc_frame.extend_from_slice(&proto_bytes);

    let req = Request::builder()
        .method("POST")
        .uri("http://localhost/hello.HelloWorldService/SayHelloStream")
        .header("Content-Type", "application/grpc")
        .header("Te", "trailers")
        .body(Full::new(Bytes::from(grpc_frame)))?;

    let resp = sender.send_request(req).await?;
    if resp.status() != 200 {
        anyhow::bail!("expected HTTP 200, got {}", resp.status());
    }

    let body = resp.into_body().collect().await?.to_bytes();

    // Parse gRPC response frames
    let mut cursor = &body[..];
    let mut msg_count = 0;
    while cursor.len() >= 5 {
        let flags = cursor[0];
        let len = u32::from_be_bytes([cursor[1], cursor[2], cursor[3], cursor[4]]) as usize;
        cursor = &cursor[5..];
        if cursor.len() < len { break; }
        let _payload = &cursor[..len];
        cursor = &cursor[len..];

        if flags & 0x80 != 0 {
            // Trailers frame
            break;
        }
        msg_count += 1;
    }

    if msg_count < 2 {
        anyhow::bail!("expected at least 2 gRPC stream messages, got {msg_count}");
    }
    Ok(())
}

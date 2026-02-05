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

pub async fn run_tonic_unary_tests(sock: &TestSocket) -> Vec<CaseResult> {
    let mut results = Vec::new();

    // Test 1: Connect unary via HTTP/1.1
    let err = test_connect_unary(sock).await.err().map(|e| e.to_string());
    results.push(CaseResult { name: "tonic unary via Connect protocol", error: err });

    // Test 2: gRPC unary via HTTP/2
    let err = test_grpc_unary(sock).await.err().map(|e| e.to_string());
    results.push(CaseResult { name: "tonic unary via gRPC protocol", error: err });

    results
}

async fn test_connect_unary(sock: &TestSocket) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("connection error: {e}");
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHello")
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .header("Host", "localhost")
        .body(Full::new(Bytes::from(r#"{"name":"Alice"}"#)))?;

    let resp = sender.send_request(req).await?;
    let status = resp.status();
    if status != 200 {
        let body = resp.into_body().collect().await?.to_bytes();
        anyhow::bail!("expected 200, got {}: {}", status, String::from_utf8_lossy(&body));
    }

    let body = resp.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)?;
    let message = json.get("message").and_then(|v| v.as_str()).unwrap_or("");
    if !message.contains("Alice") {
        anyhow::bail!("expected message to contain 'Alice', got: {message:?}");
    }
    Ok(())
}

async fn test_grpc_unary(sock: &TestSocket) -> anyhow::Result<()> {
    let (mut sender, _handle) = crate::socket::http2_connect(sock).await?;

    // Build gRPC request: protobuf-encode HelloRequest with name="Bob"
    let mut proto_bytes = Vec::new();
    // Field 1 (name), wire type 2 (length-delimited): tag = (1 << 3) | 2 = 0x0a
    let name = b"Bob";
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
        .uri("http://localhost/hello.HelloWorldService/SayHello")
        .header("Content-Type", "application/grpc")
        .header("Te", "trailers")
        .body(Full::new(Bytes::from(grpc_frame)))?;

    let resp = sender.send_request(req).await?;
    let status = resp.status();
    let grpc_status = resp.headers().get("grpc-status").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();

    let body = resp.into_body().collect().await?.to_bytes();

    // gRPC success: HTTP 200 + grpc-status 0 (might be in trailers)
    if status != 200 {
        anyhow::bail!("expected HTTP 200, got {status}");
    }

    // Parse gRPC response frame
    if body.len() < 5 {
        // grpc-status might indicate success with empty body
        if grpc_status == "0" || grpc_status.is_empty() {
            // Check if there's a valid response
            anyhow::bail!("empty gRPC response body");
        }
        anyhow::bail!("gRPC response too short: {} bytes, grpc-status: {grpc_status}", body.len());
    }

    let msg_len = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;
    if body.len() < 5 + msg_len {
        anyhow::bail!("incomplete gRPC response");
    }

    let msg_bytes = &body[5..5 + msg_len];

    // Parse protobuf HelloResponse - field 1 is message (string)
    // Simple protobuf parsing: look for tag 0x0a (field 1, wire type 2)
    let mut i = 0;
    let mut message = String::new();
    while i < msg_bytes.len() {
        let tag = msg_bytes[i];
        i += 1;
        if tag == 0x0a {
            // Length-delimited field
            let len = msg_bytes[i] as usize;
            i += 1;
            message = String::from_utf8_lossy(&msg_bytes[i..i + len]).to_string();
            break;
        }
    }

    if !message.contains("Bob") {
        anyhow::bail!("expected message to contain 'Bob', got: {message:?}");
    }
    Ok(())
}

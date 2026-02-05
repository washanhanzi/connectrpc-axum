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

pub async fn run_tonic_bidi_server_tests(sock: &TestSocket) -> Vec<CaseResult> {
    let mut results = Vec::new();

    let err = test_connect_unary(sock).await.err().map(|e| e.to_string());
    results.push(CaseResult { name: "tonic bidi server Connect unary", error: err });

    let err = test_grpc_bidi(sock).await.err().map(|e| e.to_string());
    results.push(CaseResult { name: "tonic bidi server gRPC bidi stream", error: err });

    let err = test_grpc_client_stream(sock).await.err().map(|e| e.to_string());
    results.push(CaseResult { name: "tonic bidi server gRPC client stream", error: err });

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

async fn test_grpc_bidi(sock: &TestSocket) -> anyhow::Result<()> {
    let (mut sender, _handle) = crate::socket::http2_connect(sock).await?;

    // Build 3 gRPC frames with EchoRequest messages
    let messages = [b"Hello" as &[u8], b"World", b"Bidi"];
    let mut body = Vec::new();
    for msg in &messages {
        // Protobuf: field 1 (message), wire type 2, tag = 0x0a
        let mut proto_bytes = Vec::new();
        proto_bytes.push(0x0a);
        proto_bytes.push(msg.len() as u8);
        proto_bytes.extend_from_slice(msg);

        // gRPC frame: [0x00][4-byte length][protobuf]
        body.push(0x00);
        body.extend_from_slice(&(proto_bytes.len() as u32).to_be_bytes());
        body.extend_from_slice(&proto_bytes);
    }

    let req = Request::builder()
        .method("POST")
        .uri("http://localhost/echo.EchoService/EchoBidiStream")
        .header("Content-Type", "application/grpc")
        .header("Te", "trailers")
        .body(Full::new(Bytes::from(body)))?;

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
        anyhow::bail!("expected at least 2 gRPC bidi response frames, got {msg_count}");
    }
    Ok(())
}

async fn test_grpc_client_stream(sock: &TestSocket) -> anyhow::Result<()> {
    let (mut sender, _handle) = crate::socket::http2_connect(sock).await?;

    // Build 2 gRPC frames with EchoRequest messages
    let messages = [b"Alice" as &[u8], b"Bob"];
    let mut body = Vec::new();
    for msg in &messages {
        // Protobuf: field 1 (message), wire type 2, tag = 0x0a
        let mut proto_bytes = Vec::new();
        proto_bytes.push(0x0a);
        proto_bytes.push(msg.len() as u8);
        proto_bytes.extend_from_slice(msg);

        // gRPC frame: [0x00][4-byte length][protobuf]
        body.push(0x00);
        body.extend_from_slice(&(proto_bytes.len() as u32).to_be_bytes());
        body.extend_from_slice(&proto_bytes);
    }

    let req = Request::builder()
        .method("POST")
        .uri("http://localhost/echo.EchoService/EchoClientStream")
        .header("Content-Type", "application/grpc")
        .header("Te", "trailers")
        .body(Full::new(Bytes::from(body)))?;

    let resp = sender.send_request(req).await?;
    if resp.status() != 200 {
        anyhow::bail!("expected HTTP 200, got {}", resp.status());
    }

    let body = resp.into_body().collect().await?.to_bytes();

    // Parse gRPC response frame
    if body.len() < 5 {
        anyhow::bail!("gRPC response too short: {} bytes", body.len());
    }

    let flags = body[0];
    if flags & 0x80 != 0 {
        // Trailers-only response, check for error
        let trailer_str = String::from_utf8_lossy(&body[5..]);
        anyhow::bail!("got trailers-only response: {trailer_str}");
    }

    let msg_len = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;
    if body.len() < 5 + msg_len {
        anyhow::bail!("incomplete gRPC response");
    }

    let msg_bytes = &body[5..5 + msg_len];

    // Parse protobuf EchoResponse - field 1 is message (string)
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

    if !message.contains("2 messages") {
        anyhow::bail!("expected response to contain '2 messages', got: {message:?}");
    }
    Ok(())
}

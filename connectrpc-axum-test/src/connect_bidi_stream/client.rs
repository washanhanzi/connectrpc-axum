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

fn envelope_frame(flags: u8, payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u32;
    let mut buf = Vec::with_capacity(5 + payload.len());
    buf.push(flags);
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(payload);
    buf
}

pub async fn run_bidi_stream_tests(sock: &TestSocket) -> Vec<CaseResult> {
    let err = run_one_http1(sock).await.err().map(|e| e.to_string());
    vec![CaseResult {
        name: "bidi stream echoes messages",
        error: err,
    }]
}

/// Tests bidi stream over HTTP/2 (required by connect-go servers)
pub async fn run_bidi_stream_tests_h2(sock: &TestSocket) -> Vec<CaseResult> {
    let err = run_one_http2(sock).await.err().map(|e| e.to_string());
    vec![CaseResult {
        name: "bidi stream echoes messages",
        error: err,
    }]
}

/// Bidi stream over HTTP/1.1 (half-duplex, works with Rust server)
async fn run_one_http1(sock: &TestSocket) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("connection error: {e}");
        }
    });

    let body = build_bidi_request_body();

    let req = Request::builder()
        .method("POST")
        .uri("/echo.EchoService/EchoBidiStream")
        .header("Content-Type", "application/connect+json")
        .header("Connect-Protocol-Version", "1")
        .header("Host", "localhost")
        .body(Full::new(Bytes::from(body)))?;

    let resp = sender.send_request(req).await?;
    validate_response(resp).await
}

/// Bidi stream over HTTP/2 (required by connect-go servers)
async fn run_one_http2(sock: &TestSocket) -> anyhow::Result<()> {
    let (mut sender, _handle) = crate::socket::http2_connect(sock).await?;

    let body = build_bidi_request_body();

    let req = Request::builder()
        .method("POST")
        .uri("http://localhost/echo.EchoService/EchoBidiStream")
        .header("Content-Type", "application/connect+json")
        .header("Connect-Protocol-Version", "1")
        .body(Full::new(Bytes::from(body)))?;

    let resp = sender.send_request(req).await?;
    validate_response(resp).await
}

fn build_bidi_request_body() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&envelope_frame(0x00, br#"{"message":"Hello"}"#));
    body.extend_from_slice(&envelope_frame(0x00, br#"{"message":"World"}"#));
    body.extend_from_slice(&envelope_frame(0x00, br#"{"message":"Bidi"}"#));
    body.extend_from_slice(&envelope_frame(0x02, b"{}"));
    body
}

async fn validate_response(resp: http::Response<hyper::body::Incoming>) -> anyhow::Result<()> {
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if !content_type.starts_with("application/connect+json") {
        let body_bytes = resp.into_body().collect().await?.to_bytes();
        anyhow::bail!(
            "expected content-type application/connect+json, got: {content_type} (body: {})",
            String::from_utf8_lossy(&body_bytes)
        );
    }

    let body_bytes = resp.into_body().collect().await?.to_bytes();
    let mut cursor = &body_bytes[..];
    let mut messages = Vec::new();

    while cursor.len() >= 5 {
        let flags = cursor[0];
        let len = u32::from_be_bytes([cursor[1], cursor[2], cursor[3], cursor[4]]) as usize;
        cursor = &cursor[5..];
        if cursor.len() < len {
            break;
        }
        let payload = &cursor[..len];
        cursor = &cursor[len..];

        if flags & 0x02 != 0 {
            break;
        }

        let json: serde_json::Value = serde_json::from_slice(payload)?;
        messages.push(json);
    }

    if messages.len() < 3 {
        anyhow::bail!(
            "expected at least 3 messages, got {}",
            messages.len()
        );
    }

    let first_message = messages[0]
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("expected message field in first frame, got: {}", messages[0]))?;

    if !first_message.contains("Echo #1") {
        anyhow::bail!(
            "expected first message to contain 'Echo #1', got: {:?}",
            first_message
        );
    }

    Ok(())
}

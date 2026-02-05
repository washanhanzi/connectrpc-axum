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

pub async fn run_extractor_http_response_tests(sock: &TestSocket) -> Vec<CaseResult> {
    let mut results = Vec::new();

    // Test 1: Without x-user-id header, expect HTTP 401
    let err = run_without_header(sock).await.err().map(|e| e.to_string());
    results.push(CaseResult {
        name: "without x-user-id returns 401",
        error: err,
    });

    // Test 2: With x-user-id header, expect success
    let err = run_with_header(sock).await.err().map(|e| e.to_string());
    results.push(CaseResult {
        name: "with x-user-id returns success",
        error: err,
    });

    results
}

async fn run_without_header(sock: &TestSocket) -> anyhow::Result<()> {
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

    let status = resp.status().as_u16();
    if status != 401 {
        anyhow::bail!("expected HTTP 401, got: {status}");
    }

    Ok(())
}

async fn run_with_header(sock: &TestSocket) -> anyhow::Result<()> {
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
        .header("x-user-id", "user123")
        .header("Host", "localhost")
        .body(Full::new(Bytes::from(r#"{"name":"Alice"}"#)))?;

    let resp = sender.send_request(req).await?;

    let status = resp.status().as_u16();
    if status != 200 {
        let body = resp.into_body().collect().await?.to_bytes();
        anyhow::bail!(
            "expected HTTP 200, got: {status}, body: {}",
            String::from_utf8_lossy(&body)
        );
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !content_type.starts_with("application/json") {
        anyhow::bail!(
            "expected content-type application/json, got: {content_type}"
        );
    }

    let resp_body = resp.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&resp_body)?;

    let message = json
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("expected message field, got: {json}"))?;

    if !message.contains("Alice") {
        anyhow::bail!("expected message to contain 'Alice', got: {message:?}");
    }

    if !message.contains("user123") {
        anyhow::bail!("expected message to contain 'user123', got: {message:?}");
    }

    Ok(())
}

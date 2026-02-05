use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::socket::TestSocket;

struct TestCase {
    name: &'static str,
}

const TEST_CASES: &[TestCase] = &[TestCase {
    name: "error response includes metadata",
}];

pub struct CaseResult {
    pub name: &'static str,
    pub error: Option<String>,
}

pub async fn run_unary_error_metadata_tests(sock: &TestSocket) -> Vec<CaseResult> {
    let mut results = Vec::new();
    for tc in TEST_CASES {
        let err = run_one(sock, tc).await.err().map(|e| e.to_string());
        results.push(CaseResult {
            name: tc.name,
            error: err,
        });
    }
    results
}

async fn run_one(sock: &TestSocket, _tc: &TestCase) -> anyhow::Result<()> {
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
    let headers = resp.headers().clone();
    let resp_body = resp.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&resp_body)?;

    // Validate the error code
    let code = json
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("expected code field, got: {json}"))?;
    if code != "invalid_argument" {
        anyhow::bail!("expected code invalid_argument, got {:?}", code);
    }

    // Validate the error message
    let message = json
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("expected message field, got: {json}"))?;
    if message != "name is required" {
        anyhow::bail!("expected message 'name is required', got {:?}", message);
    }

    // Validate custom metadata is present in response headers
    let custom_meta = headers
        .get("x-custom-meta")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("expected x-custom-meta header, headers: {:?}", headers))?;
    if custom_meta != "custom-value" {
        anyhow::bail!("expected x-custom-meta 'custom-value', got {:?}", custom_meta);
    }

    let request_id = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("expected x-request-id header, headers: {:?}", headers))?;
    if request_id != "test-123" {
        anyhow::bail!("expected x-request-id 'test-123', got {:?}", request_id);
    }

    Ok(())
}

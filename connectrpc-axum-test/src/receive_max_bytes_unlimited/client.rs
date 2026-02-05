use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::socket::TestSocket;

struct TestCase {
    name: &'static str,
    request_body: String,
    expect_success: bool,
}

fn test_cases() -> Vec<TestCase> {
    // Generate a ~1MB name (repeated 'A' characters)
    let large_name_1mb = "A".repeat(1_000_000);
    // Generate a ~2MB name
    let large_name_2mb = "B".repeat(2_000_000);

    vec![
        TestCase {
            name: "large unary request succeeds with unlimited receive size",
            request_body: format!(r#"{{"name":"{}"}}"#, large_name_1mb),
            expect_success: true,
        },
        TestCase {
            name: "very large unary request succeeds with unlimited receive size",
            request_body: format!(r#"{{"name":"{}"}}"#, large_name_2mb),
            expect_success: true,
        },
    ]
}

pub struct CaseResult {
    pub name: &'static str,
    pub error: Option<String>,
}

pub async fn run_receive_max_bytes_unlimited_tests(sock: &TestSocket) -> Vec<CaseResult> {
    let cases = test_cases();
    let mut results = Vec::new();
    for tc in &cases {
        let err = run_one(sock, tc).await.err().map(|e| e.to_string());
        results.push(CaseResult {
            name: tc.name,
            error: err,
        });
    }
    results
}

async fn run_one(sock: &TestSocket, tc: &TestCase) -> anyhow::Result<()> {
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
        .body(Full::new(Bytes::from(tc.request_body.clone())))?;

    let resp = sender.send_request(req).await?;
    let status = resp.status();
    let resp_body = resp.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&resp_body)?;

    if tc.expect_success {
        if status != 200 {
            anyhow::bail!("expected HTTP 200, got {status}: {json}");
        }
        let message = json
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("expected message field, got: {json}"))?;
        if message.is_empty() {
            anyhow::bail!("expected non-empty message");
        }
    } else {
        let code = json
            .get("code")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("expected error code field, got: {json}"))?;
        if code != "resource_exhausted" {
            anyhow::bail!("expected code resource_exhausted, got {:?}", code);
        }
    }

    Ok(())
}

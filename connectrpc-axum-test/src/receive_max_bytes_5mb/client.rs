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
    // Generate a name string that exceeds 5MB when serialized as JSON
    let large_name = "A".repeat(6 * 1024 * 1024);
    let large_body = format!(r#"{{"name":"{}"}}"#, large_name);

    vec![
        TestCase {
            name: "under-5MB request succeeds",
            request_body: r#"{"name":"Alice"}"#.to_string(),
            expect_success: true,
        },
        TestCase {
            name: "over-5MB request fails",
            request_body: large_body,
            expect_success: false,
        },
    ]
}

pub struct CaseResult {
    pub name: &'static str,
    pub error: Option<String>,
}

pub async fn run_receive_max_bytes_5mb_tests(sock: &TestSocket) -> Vec<CaseResult> {
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
        // Expect resource_exhausted error
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

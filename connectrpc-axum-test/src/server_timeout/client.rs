use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::socket::TestSocket;

struct TestCase {
    name: &'static str,
    timeout_ms: Option<u64>,
    expect_success: bool,
}

const TEST_CASES: &[TestCase] = &[
    TestCase {
        name: "short timeout fails",
        timeout_ms: Some(100),
        expect_success: false,
    },
    TestCase {
        name: "long timeout succeeds",
        timeout_ms: Some(1000),
        expect_success: true,
    },
    TestCase {
        name: "no timeout succeeds",
        timeout_ms: None,
        expect_success: true,
    },
];

pub struct CaseResult {
    pub name: &'static str,
    pub error: Option<String>,
}

pub async fn run_timeout_tests(sock: &TestSocket) -> Vec<CaseResult> {
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

async fn run_one(sock: &TestSocket, tc: &TestCase) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("connection error: {e}");
        }
    });

    let body = r#"{"name":"Timeout Tester"}"#;
    let mut builder = Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHello")
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .header("Host", "localhost");

    if let Some(ms) = tc.timeout_ms {
        builder = builder.header("Connect-Timeout-Ms", ms.to_string());
    }

    let req = builder.body(Full::new(Bytes::from(body)))?;
    let resp = sender.send_request(req).await?;
    let resp_body = resp.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&resp_body)?;

    if tc.expect_success {
        let message = json
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!("expected success with message, got: {json}")
            })?;
        if message.is_empty() {
            anyhow::bail!("expected non-empty message");
        }
    } else {
        let code = json
            .get("code")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!("expected error with code, got: {json}")
            })?;
        if code != "deadline_exceeded" {
            anyhow::bail!("expected deadline_exceeded, got code={code}");
        }
    }
    Ok(())
}

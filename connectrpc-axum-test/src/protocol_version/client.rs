use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::socket::TestSocket;

struct TestCase {
    name: &'static str,
    protocol_version: Option<&'static str>, // None = don't send header
    expect_success: bool,
}

const TEST_CASES: &[TestCase] = &[
    TestCase {
        name: "valid protocol version",
        protocol_version: Some("1"),
        expect_success: true,
    },
    TestCase {
        name: "missing protocol version",
        protocol_version: None,
        expect_success: false,
    },
    TestCase {
        name: "invalid protocol version",
        protocol_version: Some("2"),
        expect_success: false,
    },
];

pub struct CaseResult {
    pub name: &'static str,
    pub error: Option<String>,
}

pub async fn run_protocol_version_tests(sock: &TestSocket) -> Vec<CaseResult> {
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

    let body = r#"{"name":"Protocol Tester"}"#;
    let mut builder = Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHello")
        .header("Content-Type", "application/json")
        .header("Host", "localhost");

    if let Some(version) = tc.protocol_version {
        builder = builder.header("Connect-Protocol-Version", version);
    }

    let req = builder.body(Full::new(Bytes::from(body)))?;
    let resp = sender.send_request(req).await?;
    let status = resp.status();
    let resp_body = resp.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&resp_body)?;

    if tc.expect_success {
        if status != 200 {
            anyhow::bail!("expected HTTP 200, got {status}");
        }
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
        if status == 200 {
            anyhow::bail!("expected non-200 status for invalid protocol version, got 200 with body: {json}");
        }
        // Verify the response contains an error code
        let code = json.get("code").and_then(|v| v.as_str());
        if code.is_none() {
            anyhow::bail!("expected error with code field, got HTTP {status}: {json}");
        }
    }
    Ok(())
}

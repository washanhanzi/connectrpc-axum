use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::socket::TestSocket;

struct TestCase {
    name: &'static str,
    request_body: &'static str,
    expected_code: &'static str,
    expected_message: &'static str,
}

const TEST_CASES: &[TestCase] = &[TestCase {
    name: "error with details",
    request_body: r#"{"name":"Alice"}"#,
    expected_code: "invalid_argument",
    expected_message: "name is required",
}];

pub struct CaseResult {
    pub name: &'static str,
    pub error: Option<String>,
}

pub async fn run_error_details_tests(sock: &TestSocket) -> Vec<CaseResult> {
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

    let req = Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHello")
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .header("Host", "localhost")
        .body(Full::new(Bytes::from(tc.request_body)))?;

    let resp = sender.send_request(req).await?;
    let resp_body = resp.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&resp_body)?;

    // Validate the error code
    let code = json
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("expected code field, got: {json}"))?;
    if code != tc.expected_code {
        anyhow::bail!(
            "expected code {:?}, got {:?}",
            tc.expected_code,
            code
        );
    }

    // Validate the error message
    let message = json
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("expected message field, got: {json}"))?;
    if message != tc.expected_message {
        anyhow::bail!(
            "expected message {:?}, got {:?}",
            tc.expected_message,
            message
        );
    }

    // Validate the error details array is present and non-empty
    let details = json
        .get("details")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("expected details array, got: {json}"))?;
    if details.is_empty() {
        anyhow::bail!("expected non-empty details array, got empty");
    }

    Ok(())
}

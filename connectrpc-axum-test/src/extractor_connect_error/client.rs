use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::socket::TestSocket;

struct TestCase {
    name: &'static str,
    include_user_id: bool,
    expect_error: bool,
}

const TEST_CASES: &[TestCase] = &[
    TestCase {
        name: "without x-user-id header returns unauthenticated",
        include_user_id: false,
        expect_error: true,
    },
    TestCase {
        name: "with x-user-id header succeeds",
        include_user_id: true,
        expect_error: false,
    },
];

pub struct CaseResult {
    pub name: &'static str,
    pub error: Option<String>,
}

pub async fn run_extractor_connect_error_tests(sock: &TestSocket) -> Vec<CaseResult> {
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

    let mut builder = Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHello")
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .header("Host", "localhost");

    if tc.include_user_id {
        builder = builder.header("x-user-id", "user123");
    }

    let req = builder.body(Full::new(Bytes::from(r#"{"name":"Alice"}"#)))?;

    let resp = sender.send_request(req).await?;
    let resp_body = resp.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&resp_body)?;

    if tc.expect_error {
        // Expect a Connect error with code "unauthenticated"
        let code = json
            .get("code")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("expected code field in error response, got: {json}"))?;
        if code != "unauthenticated" {
            anyhow::bail!("expected code unauthenticated, got {:?}", code);
        }

        let message = json
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("expected message field in error response, got: {json}"))?;
        if !message.contains("x-user-id") {
            anyhow::bail!(
                "expected error message to mention x-user-id, got {:?}",
                message
            );
        }
    } else {
        // Expect success: response should contain the greeting
        let message = json
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("expected message field, got: {json}"))?;

        if !message.contains("Alice") {
            anyhow::bail!("expected message to contain 'Alice', got {:?}", message);
        }
        if !message.contains("user123") {
            anyhow::bail!("expected message to contain 'user123', got {:?}", message);
        }
    }

    Ok(())
}

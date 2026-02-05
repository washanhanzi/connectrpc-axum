use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::socket::TestSocket;

enum TestCase {
    GetPlainText {
        name: &'static str,
        path: &'static str,
        expected_body: Option<&'static str>,
    },
    PostRpc {
        name: &'static str,
        request_body: &'static str,
        expected_message: &'static str,
    },
}

const TEST_CASES: &[TestCase] = &[
    TestCase::GetPlainText {
        name: "health endpoint returns ok",
        path: "/health",
        expected_body: Some("ok"),
    },
    TestCase::GetPlainText {
        name: "metrics endpoint returns text",
        path: "/metrics",
        expected_body: None, // just check non-empty
    },
    TestCase::PostRpc {
        name: "SayHello RPC with axum routes mounted",
        request_body: r#"{"name":"Alice"}"#,
        expected_message: "Hello, Alice!",
    },
];

pub struct CaseResult {
    pub name: &'static str,
    pub error: Option<String>,
}

pub async fn run_axum_router_tests(sock: &TestSocket) -> Vec<CaseResult> {
    let mut results = Vec::new();
    for tc in TEST_CASES {
        let (name, err) = match tc {
            TestCase::GetPlainText {
                name,
                path,
                expected_body,
            } => (
                *name,
                run_get(sock, path, *expected_body)
                    .await
                    .err()
                    .map(|e| e.to_string()),
            ),
            TestCase::PostRpc {
                name,
                request_body,
                expected_message,
            } => (
                *name,
                run_rpc(sock, request_body, expected_message)
                    .await
                    .err()
                    .map(|e| e.to_string()),
            ),
        };
        results.push(CaseResult { name, error: err });
    }
    results
}

async fn run_get(
    sock: &TestSocket,
    path: &str,
    expected_body: Option<&str>,
) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("connection error: {e}");
        }
    });

    let req = Request::builder()
        .method("GET")
        .uri(path)
        .header("Host", "localhost")
        .body(Full::new(Bytes::new()))?;

    let resp = sender.send_request(req).await?;

    let status = resp.status().as_u16();
    if status != 200 {
        anyhow::bail!("expected status 200, got {status}");
    }

    let resp_body = resp.into_body().collect().await?.to_bytes();
    let body_str = String::from_utf8_lossy(&resp_body);

    if body_str.is_empty() {
        anyhow::bail!("expected non-empty body");
    }

    if let Some(expected) = expected_body {
        if body_str.as_ref() != expected {
            anyhow::bail!("expected body {:?}, got {:?}", expected, body_str);
        }
    }

    Ok(())
}

async fn run_rpc(
    sock: &TestSocket,
    request_body: &str,
    expected_message: &str,
) -> anyhow::Result<()> {
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
        .body(Full::new(Bytes::from(request_body.to_owned())))?;

    let resp = sender.send_request(req).await?;

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

    if message != expected_message {
        anyhow::bail!(
            "expected message {:?}, got {:?}",
            expected_message,
            message
        );
    }

    Ok(())
}

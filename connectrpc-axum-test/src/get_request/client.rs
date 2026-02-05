use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::socket::TestSocket;

struct TestCase {
    name: &'static str,
    uri: &'static str,
    expect_success: bool,
    expected_message: Option<&'static str>,
}

const TEST_CASES: &[TestCase] = &[
    TestCase {
        name: "GET with JSON encoding and name",
        // For encoding=json, the message param is raw JSON (URL-encoded in query string)
        uri: "/hello.HelloWorldService/GetGreeting?encoding=json&connect=v1&message=%7B%22name%22%3A%22Alice%22%7D",
        expect_success: true,
        expected_message: Some("Hello, Alice!"),
    },
    TestCase {
        name: "GET with JSON encoding and no name",
        // message={} as raw JSON in query string
        uri: "/hello.HelloWorldService/GetGreeting?encoding=json&connect=v1&message=%7B%7D",
        expect_success: true,
        expected_message: Some("Hello, World!"),
    },
    TestCase {
        name: "GET with base64-encoded JSON message",
        // encoding=json with base64=1: message is base64url-encoded JSON
        // {"name":"Alice"} -> base64url: eyJuYW1lIjoiQWxpY2UifQ
        uri: "/hello.HelloWorldService/GetGreeting?encoding=json&connect=v1&base64=1&message=eyJuYW1lIjoiQWxpY2UifQ",
        expect_success: true,
        expected_message: Some("Hello, Alice!"),
    },
    TestCase {
        name: "GET missing message parameter",
        uri: "/hello.HelloWorldService/GetGreeting?encoding=json&connect=v1",
        expect_success: false,
        expected_message: None,
    },
];

pub struct CaseResult {
    pub name: &'static str,
    pub error: Option<String>,
}

pub async fn run_get_request_tests(sock: &TestSocket) -> Vec<CaseResult> {
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
        .method("GET")
        .uri(tc.uri)
        .header("Host", "localhost")
        .body(Full::new(Bytes::new()))?;

    let resp = sender.send_request(req).await?;
    let status = resp.status();
    let resp_body = resp.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&resp_body)?;

    if tc.expect_success {
        if status != 200 {
            anyhow::bail!("expected HTTP 200, got {status}: {json}");
        }
        if let Some(expected) = tc.expected_message {
            let message = json
                .get("message")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("expected message field, got: {json}"))?;
            if message != expected {
                anyhow::bail!("expected message {:?}, got {:?}", expected, message);
            }
        }
    } else {
        if status == 200 {
            anyhow::bail!("expected non-200 status, got 200: {json}");
        }
    }

    Ok(())
}

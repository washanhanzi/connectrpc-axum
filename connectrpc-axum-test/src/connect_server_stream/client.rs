use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::socket::TestSocket;

struct TestCase {
    name: &'static str,
    request_body: &'static str,
    expected_first_message_contains: &'static str,
    expected_min_messages: usize,
}

const TEST_CASES: &[TestCase] = &[TestCase {
    name: "server stream returns messages",
    request_body: r#"{"name":"Stream Tester"}"#,
    expected_first_message_contains: "Hello",
    expected_min_messages: 2,
}];

pub struct CaseResult {
    pub name: &'static str,
    pub error: Option<String>,
}

/// Wrap a JSON payload in the Connect streaming envelope format:
/// [1 byte flags][4 bytes big-endian length][payload]
fn envelope_frame(payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u32;
    let mut buf = Vec::with_capacity(5 + payload.len());
    buf.push(0x00); // flags: data frame
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(payload);
    buf
}

pub async fn run_server_stream_tests(sock: &TestSocket) -> Vec<CaseResult> {
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

    let enveloped = envelope_frame(tc.request_body.as_bytes());

    let req = Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHelloStream")
        .header("Content-Type", "application/connect+json")
        .header("Connect-Protocol-Version", "1")
        .header("Host", "localhost")
        .body(Full::new(Bytes::from(enveloped)))?;

    let resp = sender.send_request(req).await?;

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if !content_type.starts_with("application/connect+json") {
        let body_bytes = resp.into_body().collect().await?.to_bytes();
        let body_str = String::from_utf8_lossy(&body_bytes);
        anyhow::bail!(
            "expected content-type application/connect+json, got: {content_type} (body: {body_str})"
        );
    }

    let body_bytes = resp.into_body().collect().await?.to_bytes();
    let mut cursor = &body_bytes[..];
    let mut messages = Vec::new();

    while cursor.len() >= 5 {
        let flags = cursor[0];
        let len =
            u32::from_be_bytes([cursor[1], cursor[2], cursor[3], cursor[4]]) as usize;
        cursor = &cursor[5..];
        if cursor.len() < len {
            break;
        }
        let payload = &cursor[..len];
        cursor = &cursor[len..];

        if flags & 0x02 != 0 {
            // End-of-stream trailer
            break;
        }

        let json: serde_json::Value = serde_json::from_slice(payload)?;
        messages.push(json);
    }

    if messages.len() < tc.expected_min_messages {
        anyhow::bail!(
            "expected at least {} messages, got {}",
            tc.expected_min_messages,
            messages.len()
        );
    }

    let first_message = messages[0]
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            anyhow::anyhow!("expected message field in first frame, got: {}", messages[0])
        })?;

    if !first_message.contains(tc.expected_first_message_contains) {
        anyhow::bail!(
            "expected first message to contain {:?}, got {:?}",
            tc.expected_first_message_contains,
            first_message
        );
    }

    Ok(())
}

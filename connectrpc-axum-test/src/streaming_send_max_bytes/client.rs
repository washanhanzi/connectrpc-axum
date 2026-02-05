use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::socket::TestSocket;

struct TestCase {
    name: &'static str,
    request_name: &'static str,
    expect_success: bool,
    expected_min_messages: usize,
}

const TEST_CASES: &[TestCase] = &[
    TestCase {
        name: "streaming with small responses succeeds under send limit",
        request_name: "Small",
        expect_success: true,
        expected_min_messages: 2,
    },
    TestCase {
        name: "streaming fails when response message exceeds send limit",
        request_name: "Large",
        expect_success: false,
        expected_min_messages: 0,
    },
];

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

pub struct CaseResult {
    pub name: &'static str,
    pub error: Option<String>,
}

pub async fn run_streaming_send_max_bytes_tests(sock: &TestSocket) -> Vec<CaseResult> {
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

    let request_body = format!(r#"{{"name":"{}"}}"#, tc.request_name);
    let enveloped = envelope_frame(request_body.as_bytes());

    let req = Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHelloStream")
        .header("Content-Type", "application/connect+json")
        .header("Connect-Protocol-Version", "1")
        .header("Host", "localhost")
        .body(Full::new(Bytes::from(enveloped)))?;

    let resp = sender.send_request(req).await?;

    let status = resp.status();
    if status != 200 {
        let body_bytes = resp.into_body().collect().await?.to_bytes();
        let body_str = String::from_utf8_lossy(&body_bytes);
        anyhow::bail!("expected HTTP 200, got {status}: {body_str}");
    }

    let body_bytes = resp.into_body().collect().await?.to_bytes();

    // Parse binary-framed stream: [1 byte flags][4 bytes BE length][payload]
    let mut cursor = &body_bytes[..];
    let mut messages = Vec::new();
    let mut end_stream_error: Option<serde_json::Value> = None;

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
            // End-of-stream trailer frame
            let json: serde_json::Value = serde_json::from_slice(payload)?;
            end_stream_error = Some(json);
            break;
        }

        let json: serde_json::Value = serde_json::from_slice(payload)?;
        messages.push(json);
    }

    if tc.expect_success {
        // Should have received all messages without error
        if messages.len() < tc.expected_min_messages {
            anyhow::bail!(
                "expected at least {} messages, got {}",
                tc.expected_min_messages,
                messages.len()
            );
        }

        // If there is an EndStream frame, check it has no error
        if let Some(end_stream) = &end_stream_error {
            if end_stream.get("error").is_some() {
                anyhow::bail!(
                    "expected no error in EndStream, got: {end_stream}"
                );
            }
        }
    } else {
        // Should have a resource_exhausted error.
        // Rust server: EndStream frame with resource_exhausted error (EndStream is exempt from send_max_bytes).
        // Go server: EndStream frame may be absent because connect-go applies sendMaxBytes to
        // EndStream frames too, causing the error EndStream (~90 bytes) to exceed the 64-byte limit.
        // In that case, the body is empty (no data messages, no EndStream).
        if let Some(end_stream) = &end_stream_error {
            let error_obj = end_stream.get("error").ok_or_else(|| {
                anyhow::anyhow!("expected error field in EndStream, got: {end_stream}")
            })?;

            let code = error_obj
                .get("code")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("expected code in error, got: {error_obj}"))?;

            if code != "resource_exhausted" {
                anyhow::bail!("expected error code 'resource_exhausted', got {:?}", code);
            }
        } else if !messages.is_empty() {
            // If there's no EndStream but there ARE data messages, something is wrong
            anyhow::bail!(
                "expected resource_exhausted error, got {} data messages and no EndStream",
                messages.len()
            );
        }
        // else: empty body (no messages, no EndStream) — acceptable for Go server
    }

    Ok(())
}

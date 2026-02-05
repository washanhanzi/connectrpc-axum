use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::socket::TestSocket;

struct TestCase {
    name: &'static str,
    api_key: Option<&'static str>,
    expect_error: bool,
    expected_error_code: Option<&'static str>,
    expected_min_messages: usize,
    expected_first_message_contains: Option<&'static str>,
}

const TEST_CASES: &[TestCase] = &[
    TestCase {
        name: "streaming without x-api-key returns unauthenticated",
        api_key: None,
        expect_error: true,
        expected_error_code: Some("unauthenticated"),
        expected_min_messages: 0,
        expected_first_message_contains: None,
    },
    TestCase {
        name: "streaming with x-api-key succeeds",
        api_key: Some("test-key"),
        expect_error: false,
        expected_error_code: None,
        expected_min_messages: 2,
        expected_first_message_contains: Some("Hello"),
    },
];

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

pub async fn run_streaming_extractor_tests(sock: &TestSocket) -> Vec<CaseResult> {
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

    let request_body = r#"{"name":"Extractor Tester"}"#;
    let enveloped = envelope_frame(request_body.as_bytes());

    let mut builder = Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHelloStream")
        .header("Content-Type", "application/connect+json")
        .header("Connect-Protocol-Version", "1")
        .header("Host", "localhost");

    if let Some(key) = tc.api_key {
        builder = builder.header("x-api-key", key);
    }

    let req = builder.body(Full::new(Bytes::from(enveloped)))?;
    let resp = sender.send_request(req).await?;

    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body_bytes = resp.into_body().collect().await?.to_bytes();

    if tc.expect_error {
        // The error can appear either as:
        // 1. A non-200 HTTP status with a JSON error body (unary-style error)
        // 2. A 200 response with an EndStream frame containing the error

        if status != 200 {
            // Unary-style error response (non-streaming)
            let body_str = String::from_utf8_lossy(&body_bytes);
            let json: serde_json::Value = serde_json::from_slice(&body_bytes)
                .map_err(|_| anyhow::anyhow!("expected JSON error body, got: {body_str}"))?;

            let code = json
                .get("code")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("expected code field in error, got: {json}"))?;
            if let Some(expected) = tc.expected_error_code {
                if code != expected {
                    anyhow::bail!("expected error code '{expected}', got '{code}'");
                }
            }
            return Ok(());
        }

        // 200 response -- look for error in EndStream frame
        let mut cursor = &body_bytes[..];
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
                if let Some(error_obj) = json.get("error") {
                    let code = error_obj
                        .get("code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if let Some(expected) = tc.expected_error_code {
                        if code != expected {
                            anyhow::bail!("expected error code '{expected}', got '{code}'");
                        }
                    }
                    return Ok(());
                }
            }
        }

        anyhow::bail!(
            "expected error response, got HTTP {status} with body: {}",
            String::from_utf8_lossy(&body_bytes)
        );
    }

    // Success case -- expect streaming response with messages
    if !content_type.starts_with("application/connect+json") {
        anyhow::bail!(
            "expected content-type application/connect+json, got: {content_type} (body: {})",
            String::from_utf8_lossy(&body_bytes)
        );
    }

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

    if let Some(expected) = tc.expected_first_message_contains {
        let first_message = messages[0]
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!("expected message field in first frame, got: {}", messages[0])
            })?;

        if !first_message.contains(expected) {
            anyhow::bail!(
                "expected first message to contain {:?}, got {:?}",
                expected,
                first_message
            );
        }
    }

    Ok(())
}

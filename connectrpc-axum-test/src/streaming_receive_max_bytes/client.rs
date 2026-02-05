use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::socket::TestSocket;

struct TestCase {
    name: &'static str,
    request_body: &'static str,
    expect_success: bool,
}

const TEST_CASES: &[TestCase] = &[
    TestCase {
        name: "small streaming request succeeds",
        request_body: r#"{"name":"Alice"}"#,
        expect_success: true,
    },
    TestCase {
        name: "large streaming request fails",
        request_body: r#"{"name":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}"#,
        expect_success: false,
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

pub async fn run_streaming_receive_max_bytes_tests(sock: &TestSocket) -> Vec<CaseResult> {
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
    let status = resp.status();
    let body_bytes = resp.into_body().collect().await?.to_bytes();

    if tc.expect_success {
        if status != 200 {
            anyhow::bail!("expected HTTP 200, got {status}: {}", String::from_utf8_lossy(&body_bytes));
        }

        // Parse streaming response frames
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

        if messages.is_empty() {
            anyhow::bail!("expected at least 1 message, got 0");
        }

        let first_message = messages[0]
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!("expected message field in first frame, got: {}", messages[0])
            })?;

        if !first_message.contains("Hello") {
            anyhow::bail!(
                "expected first message to contain 'Hello', got {:?}",
                first_message
            );
        }
    } else {
        // For streaming, the error may come as:
        // 1. An HTTP-level error with a JSON body (non-200 status)
        // 2. An EndStream frame with error inside (HTTP 200)
        if status == 200 {
            // Parse streaming response looking for EndStream frame with error
            let mut cursor = &body_bytes[..];
            let mut found_error = false;

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
                    let error_obj = json
                        .get("error")
                        .ok_or_else(|| {
                            anyhow::anyhow!("expected error in EndStream, got: {json}")
                        })?;
                    let code = error_obj
                        .get("code")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            anyhow::anyhow!("expected code in error, got: {error_obj}")
                        })?;
                    if code != "resource_exhausted" {
                        anyhow::bail!(
                            "expected code resource_exhausted, got {:?}",
                            code
                        );
                    }
                    found_error = true;
                    break;
                }
            }

            if !found_error {
                anyhow::bail!(
                    "expected resource_exhausted error in EndStream frame, got body: {}",
                    String::from_utf8_lossy(&body_bytes)
                );
            }
        } else {
            // Non-200: parse as unary-style JSON error
            let json: serde_json::Value = serde_json::from_slice(&body_bytes)
                .map_err(|_| {
                    anyhow::anyhow!(
                        "expected JSON error body, got: {}",
                        String::from_utf8_lossy(&body_bytes)
                    )
                })?;
            let code = json
                .get("code")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("expected error code field, got: {json}"))?;
            if code != "resource_exhausted" {
                anyhow::bail!("expected code resource_exhausted, got {:?}", code);
            }
        }
    }

    Ok(())
}

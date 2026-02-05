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
}

const TEST_CASES: &[TestCase] = &[
    TestCase {
        name: "client streaming without x-api-key returns unauthenticated",
        api_key: None,
        expect_error: true,
        expected_error_code: Some("unauthenticated"),
    },
    TestCase {
        name: "client streaming with x-api-key succeeds",
        api_key: Some("test-key"),
        expect_error: false,
        expected_error_code: None,
    },
];

pub struct CaseResult {
    pub name: &'static str,
    pub error: Option<String>,
}

fn envelope_frame(flags: u8, payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u32;
    let mut buf = Vec::with_capacity(5 + payload.len());
    buf.push(flags);
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(payload);
    buf
}

pub async fn run_streaming_extractor_client_tests(sock: &TestSocket) -> Vec<CaseResult> {
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
    tokio::spawn(async move { if let Err(e) = conn.await { eprintln!("connection error: {e}"); } });

    let mut body = Vec::new();
    body.extend_from_slice(&envelope_frame(0x00, br#"{"message":"Hello"}"#));
    body.extend_from_slice(&envelope_frame(0x00, br#"{"message":"World"}"#));
    body.extend_from_slice(&envelope_frame(0x02, b"{}"));

    let mut builder = Request::builder()
        .method("POST")
        .uri("/echo.EchoService/EchoClientStream")
        .header("Content-Type", "application/connect+json")
        .header("Connect-Protocol-Version", "1")
        .header("Host", "localhost");

    if let Some(key) = tc.api_key {
        builder = builder.header("x-api-key", key);
    }

    let req = builder.body(Full::new(Bytes::from(body)))?;
    let resp = sender.send_request(req).await?;
    let status = resp.status().as_u16();
    let body_bytes = resp.into_body().collect().await?.to_bytes();

    if tc.expect_error {
        if status != 200 {
            let json: serde_json::Value = serde_json::from_slice(&body_bytes)
                .map_err(|_| anyhow::anyhow!("expected JSON error body, got: {}", String::from_utf8_lossy(&body_bytes)))?;
            let code = json.get("code").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(expected) = tc.expected_error_code {
                if code != expected {
                    anyhow::bail!("expected error code '{expected}', got '{code}'");
                }
            }
            return Ok(());
        }

        // Check EndStream frame for error
        let mut cursor = &body_bytes[..];
        while cursor.len() >= 5 {
            let flags = cursor[0];
            let len = u32::from_be_bytes([cursor[1], cursor[2], cursor[3], cursor[4]]) as usize;
            cursor = &cursor[5..];
            if cursor.len() < len { break; }
            let payload = &cursor[..len];
            cursor = &cursor[len..];
            if flags & 0x02 != 0 {
                let json: serde_json::Value = serde_json::from_slice(payload)?;
                if let Some(error_obj) = json.get("error") {
                    let code = error_obj.get("code").and_then(|v| v.as_str()).unwrap_or("");
                    if let Some(expected) = tc.expected_error_code {
                        if code != expected {
                            anyhow::bail!("expected error code '{expected}', got '{code}'");
                        }
                    }
                    return Ok(());
                }
            }
        }
        anyhow::bail!("expected error response, got HTTP {status}: {}", String::from_utf8_lossy(&body_bytes));
    }

    // Success case
    if status != 200 {
        anyhow::bail!("expected 200, got {status}: {}", String::from_utf8_lossy(&body_bytes));
    }

    // Parse response (streaming or plain JSON)
    let response_text = if body_bytes.len() >= 5 && (body_bytes[0] == 0x00 || body_bytes[0] == 0x01) {
        let mut cursor = &body_bytes[..];
        let mut text = String::new();
        while cursor.len() >= 5 {
            let flags = cursor[0];
            let len = u32::from_be_bytes([cursor[1], cursor[2], cursor[3], cursor[4]]) as usize;
            cursor = &cursor[5..];
            if cursor.len() < len { break; }
            let payload = &cursor[..len];
            cursor = &cursor[len..];
            if flags & 0x02 != 0 { break; }
            if flags == 0x00 {
                let json: serde_json::Value = serde_json::from_slice(payload)?;
                if let Some(msg) = json.get("message").and_then(|v| v.as_str()) {
                    text = msg.to_string();
                }
            }
        }
        text
    } else {
        let json: serde_json::Value = serde_json::from_slice(&body_bytes)?;
        json.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string()
    };

    if !response_text.contains("2 messages") {
        anyhow::bail!("expected '2 messages', got: {response_text:?}");
    }

    Ok(())
}

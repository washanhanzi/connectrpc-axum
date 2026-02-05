use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::socket::TestSocket;

struct TestCase {
    name: &'static str,
}

const TEST_CASES: &[TestCase] = &[TestCase {
    name: "streaming error in EndStream frame",
}];

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

pub async fn run_streaming_error_tests(sock: &TestSocket) -> Vec<CaseResult> {
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

async fn run_one(sock: &TestSocket, _tc: &TestCase) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("connection error: {e}");
        }
    });

    let request_body = r#"{"name":"Error Tester"}"#;
    let enveloped = envelope_frame(request_body.as_bytes());

    let req = Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHelloStream")
        .header("Content-Type", "application/connect+json")
        .header("Connect-Protocol-Version", "1")
        .header("Host", "localhost")
        .body(Full::new(Bytes::from(enveloped)))?;

    let resp = sender.send_request(req).await?;

    // HTTP status should be 200 for streaming errors
    let status = resp.status();
    if status != 200 {
        anyhow::bail!("expected HTTP 200 for streaming response, got {status}");
    }

    let body_bytes = resp.into_body().collect().await?.to_bytes();

    // Parse binary-framed stream looking for the EndStream frame
    let mut cursor = &body_bytes[..];
    let mut found_end_stream = false;
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
            found_end_stream = true;
            let json: serde_json::Value = serde_json::from_slice(payload)?;
            end_stream_error = Some(json);
            break;
        }
    }

    if !found_end_stream {
        anyhow::bail!(
            "expected EndStream frame in response, got body: {}",
            String::from_utf8_lossy(&body_bytes)
        );
    }

    let error_json = end_stream_error.unwrap();
    let error_obj = error_json
        .get("error")
        .ok_or_else(|| anyhow::anyhow!("expected error field in EndStream, got: {error_json}"))?;

    let code = error_obj
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("expected code in error, got: {error_obj}"))?;
    if code != "internal" {
        anyhow::bail!("expected error code 'internal', got {:?}", code);
    }

    let message = error_obj
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("expected message in error, got: {error_obj}"))?;
    if message != "something went wrong" {
        anyhow::bail!(
            "expected error message 'something went wrong', got {:?}",
            message
        );
    }

    Ok(())
}

use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::socket::TestSocket;

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

pub async fn run_client_stream_tests(sock: &TestSocket) -> Vec<CaseResult> {
    let err = run_one(sock).await.err().map(|e| e.to_string());
    vec![CaseResult {
        name: "client stream aggregates messages",
        error: err,
    }]
}

async fn run_one(sock: &TestSocket) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("connection error: {e}");
        }
    });

    // Build request body: 3 message frames + EndStream
    let mut body = Vec::new();
    body.extend_from_slice(&envelope_frame(0x00, br#"{"message":"Hello"}"#));
    body.extend_from_slice(&envelope_frame(0x00, br#"{"message":"World"}"#));
    body.extend_from_slice(&envelope_frame(0x00, br#"{"message":"Test"}"#));
    body.extend_from_slice(&envelope_frame(0x02, b"{}"));

    let req = Request::builder()
        .method("POST")
        .uri("/echo.EchoService/EchoClientStream")
        .header("Content-Type", "application/connect+json")
        .header("Connect-Protocol-Version", "1")
        .header("Host", "localhost")
        .body(Full::new(Bytes::from(body)))?;

    let resp = sender.send_request(req).await?;
    let status = resp.status().as_u16();

    if status != 200 {
        let body_bytes = resp.into_body().collect().await?.to_bytes();
        anyhow::bail!(
            "expected HTTP 200, got {status}: {}",
            String::from_utf8_lossy(&body_bytes)
        );
    }

    let body_bytes = resp.into_body().collect().await?.to_bytes();

    // Response may be streaming format (envelope-framed) or plain JSON
    let response_text = if body_bytes.len() >= 5 && (body_bytes[0] == 0x00 || body_bytes[0] == 0x01) {
        // Streaming format - extract first data frame
        let mut cursor = &body_bytes[..];
        let mut text = String::new();
        while cursor.len() >= 5 {
            let flags = cursor[0];
            let len = u32::from_be_bytes([cursor[1], cursor[2], cursor[3], cursor[4]]) as usize;
            cursor = &cursor[5..];
            if cursor.len() < len {
                break;
            }
            let payload = &cursor[..len];
            cursor = &cursor[len..];

            if flags & 0x02 != 0 {
                break;
            }
            if flags == 0x00 {
                let json: serde_json::Value = serde_json::from_slice(payload)?;
                if let Some(msg) = json.get("message").and_then(|v| v.as_str()) {
                    text = msg.to_string();
                }
            }
        }
        text
    } else {
        // Plain JSON response
        let json: serde_json::Value = serde_json::from_slice(&body_bytes)?;
        json.get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };

    if !response_text.contains("3 messages") {
        anyhow::bail!(
            "expected response to mention '3 messages', got: {:?}",
            response_text
        );
    }
    for name in &["Hello", "World", "Test"] {
        if !response_text.contains(name) {
            anyhow::bail!(
                "expected response to contain {:?}, got: {:?}",
                name,
                response_text
            );
        }
    }

    Ok(())
}

use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use std::io::{Read, Write};

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

fn decompress_gzip(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut decoder = flate2::read::GzDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

fn compress_gzip(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

pub async fn run_client_streaming_compression_tests(sock: &TestSocket) -> Vec<CaseResult> {
    let err = run_one(sock, false).await.err().map(|e| e.to_string());
    vec![CaseResult {
        name: "compressed client stream frames are decompressed",
        error: err,
    }]
}

/// Variant for Go server: all frames must be compressed when Connect-Content-Encoding is set
pub async fn run_client_streaming_compression_tests_go(sock: &TestSocket) -> Vec<CaseResult> {
    let err = run_one(sock, true).await.err().map(|e| e.to_string());
    vec![CaseResult {
        name: "compressed client stream frames are decompressed",
        error: err,
    }]
}

async fn run_one(sock: &TestSocket, all_compressed: bool) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("connection error: {e}");
        }
    });

    // Frame 1: uncompressed (or compressed for Go server)
    let payload1 = br#"{"message":"Hello from Alice"}"#;
    let frame1 = if all_compressed {
        let compressed1 = compress_gzip(payload1)?;
        envelope_frame(0x01, &compressed1)
    } else {
        envelope_frame(0x00, payload1)
    };

    // Frame 2: compressed
    let payload2 = br#"{"message":"Hello from Bob via compressed frame"}"#;
    let compressed2 = compress_gzip(payload2)?;
    let frame2 = envelope_frame(0x01, &compressed2);

    // Frame 3: compressed
    let payload3 = br#"{"message":"Hello from Charlie also compressed"}"#;
    let compressed3 = compress_gzip(payload3)?;
    let frame3 = envelope_frame(0x01, &compressed3);

    // EndStream
    let end_frame = envelope_frame(0x02, b"{}");

    let mut body = Vec::new();
    body.extend_from_slice(&frame1);
    body.extend_from_slice(&frame2);
    body.extend_from_slice(&frame3);
    body.extend_from_slice(&end_frame);

    let req = Request::builder()
        .method("POST")
        .uri("/echo.EchoService/EchoClientStream")
        .header("Content-Type", "application/connect+json")
        .header("Connect-Protocol-Version", "1")
        .header("Connect-Content-Encoding", "gzip")
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

    // Parse response (may be streaming or plain JSON)
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
            let json_bytes = if flags & 0x01 != 0 {
                decompress_gzip(payload)?
            } else {
                payload.to_vec()
            };
            let json: serde_json::Value = serde_json::from_slice(&json_bytes)?;
            if let Some(msg) = json.get("message").and_then(|v| v.as_str()) {
                text = msg.to_string();
            }
        }
        text
    } else {
        let json: serde_json::Value = serde_json::from_slice(&body_bytes)?;
        json.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string()
    };

    if !response_text.contains("3 messages") {
        anyhow::bail!("expected '3 messages' in response, got: {:?}", response_text);
    }
    for name in &["Alice", "Bob", "Charlie"] {
        if !response_text.contains(name) {
            anyhow::bail!("expected {:?} in response, got: {:?}", name, response_text);
        }
    }

    Ok(())
}

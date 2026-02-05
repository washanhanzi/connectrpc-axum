use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use std::io::Read;

use crate::socket::TestSocket;

pub struct CaseResult {
    pub name: &'static str,
    pub error: Option<String>,
}

fn envelope_frame(payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u32;
    let mut buf = Vec::with_capacity(5 + payload.len());
    buf.push(0x00);
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

pub async fn run_streaming_compression_tests(sock: &TestSocket) -> Vec<CaseResult> {
    let err = run_one(sock, true).await.err().map(|e| e.to_string());
    vec![CaseResult {
        name: "server stream messages are compressed with gzip",
        error: err,
    }]
}

/// Same as above but does not require uncompressed frames
/// (connect-go compresses all frames regardless of size)
pub async fn run_streaming_compression_tests_go(sock: &TestSocket) -> Vec<CaseResult> {
    let err = run_one(sock, false).await.err().map(|e| e.to_string());
    vec![CaseResult {
        name: "server stream messages are compressed with gzip",
        error: err,
    }]
}

async fn run_one(sock: &TestSocket, expect_mixed: bool) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("connection error: {e}");
        }
    });

    let enveloped = envelope_frame(br#"{"name":"TestUser"}"#);

    let req = Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHelloStream")
        .header("Content-Type", "application/connect+json")
        .header("Connect-Protocol-Version", "1")
        .header("Connect-Accept-Encoding", "gzip")
        .header("Host", "localhost")
        .body(Full::new(Bytes::from(enveloped)))?;

    let resp = sender.send_request(req).await?;

    let connect_content_encoding = resp
        .headers()
        .get("connect-content-encoding")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if connect_content_encoding != "gzip" {
        anyhow::bail!(
            "expected Connect-Content-Encoding: gzip, got: {:?}",
            connect_content_encoding
        );
    }

    let body_bytes = resp.into_body().collect().await?.to_bytes();
    let mut cursor = &body_bytes[..];
    let mut compressed_count = 0;
    let mut uncompressed_count = 0;

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

        if flags & 0x01 != 0 {
            compressed_count += 1;
            let decompressed = decompress_gzip(payload)?;
            let _json: serde_json::Value = serde_json::from_slice(&decompressed)?;
        } else {
            uncompressed_count += 1;
            let _json: serde_json::Value = serde_json::from_slice(payload)?;
        }
    }

    if compressed_count == 0 {
        anyhow::bail!("expected at least 1 compressed frame (flag 0x01), got 0");
    }

    if expect_mixed && uncompressed_count == 0 {
        anyhow::bail!("expected at least 1 uncompressed frame (flag 0x00), got 0");
    }

    Ok(())
}

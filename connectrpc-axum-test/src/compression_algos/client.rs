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

fn compress_deflate(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

fn decompress_deflate(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut decoder = flate2::read::ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

fn compress_brotli_data(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut output = Vec::new();
    {
        let mut encoder = brotli::CompressorWriter::new(&mut output, 4096, 6, 22);
        encoder.write_all(data)?;
    }
    Ok(output)
}

fn decompress_brotli_data(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut decoder = brotli::Decompressor::new(data, 4096);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

fn compress_zstd_data(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    Ok(zstd::encode_all(data, 3)?)
}

fn decompress_zstd_data(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    Ok(zstd::decode_all(data)?)
}

fn compress_gzip(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

fn decompress_gzip(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut decoder = flate2::read::GzDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

type CompressFn = fn(&[u8]) -> anyhow::Result<Vec<u8>>;
type DecompressFn = fn(&[u8]) -> anyhow::Result<Vec<u8>>;

struct AlgoTestCase {
    name: &'static str,
    encoding: &'static str,
    compress: CompressFn,
    decompress: DecompressFn,
}

pub async fn run_compression_algos_tests(sock: &TestSocket) -> Vec<CaseResult> {
    let algos = [
        AlgoTestCase {
            name: "deflate",
            encoding: "deflate",
            compress: compress_deflate,
            decompress: decompress_deflate,
        },
        AlgoTestCase {
            name: "brotli",
            encoding: "br",
            compress: compress_brotli_data,
            decompress: decompress_brotli_data,
        },
        AlgoTestCase {
            name: "zstd",
            encoding: "zstd",
            compress: compress_zstd_data,
            decompress: decompress_zstd_data,
        },
        AlgoTestCase {
            name: "gzip",
            encoding: "gzip",
            compress: compress_gzip,
            decompress: decompress_gzip,
        },
    ];

    let mut results = Vec::new();

    // Streaming response compression
    for algo in &algos {
        if algo.name == "gzip" {
            continue;
        } // gzip covered by streaming_compression_gzip
        let test_name = format!("streaming response compressed with {}", algo.name);
        let err = test_streaming_response(sock, algo.encoding, algo.decompress)
            .await
            .err()
            .map(|e| e.to_string());
        results.push(CaseResult {
            name: Box::leak(test_name.into_boxed_str()),
            error: err,
        });
    }

    // Client streaming decompression
    for algo in &algos {
        if algo.name == "gzip" {
            continue;
        } // gzip covered by client_streaming_compression
        let test_name = format!(
            "client stream compressed with {} is decompressed",
            algo.name
        );
        let err = test_client_streaming(sock, algo.encoding, algo.compress)
            .await
            .err()
            .map(|e| e.to_string());
        results.push(CaseResult {
            name: Box::leak(test_name.into_boxed_str()),
            error: err,
        });
    }

    // Unary compression
    for algo in &algos {
        let test_name = format!("unary {} compression works end to end", algo.name);
        let err = test_unary_compression(sock, algo.encoding, algo.compress, algo.decompress)
            .await
            .err()
            .map(|e| e.to_string());
        results.push(CaseResult {
            name: Box::leak(test_name.into_boxed_str()),
            error: err,
        });
    }

    results
}

async fn test_streaming_response(
    sock: &TestSocket,
    encoding: &str,
    decompress: DecompressFn,
) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("connection error: {e}");
        }
    });

    let enveloped = envelope_frame(0x00, br#"{"name":"TestUser"}"#);
    let req = Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHelloStream")
        .header("Content-Type", "application/connect+json")
        .header("Connect-Protocol-Version", "1")
        .header("Connect-Accept-Encoding", encoding)
        .header("Host", "localhost")
        .body(Full::new(Bytes::from(enveloped)))?;

    let resp = sender.send_request(req).await?;
    let connect_encoding = resp
        .headers()
        .get("connect-content-encoding")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if connect_encoding != encoding {
        anyhow::bail!("expected Connect-Content-Encoding: {encoding}, got: {connect_encoding:?}");
    }

    let body_bytes = resp.into_body().collect().await?.to_bytes();
    let mut cursor = &body_bytes[..];
    let mut compressed_count = 0;

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
            let decompressed = decompress(payload)?;
            let _: serde_json::Value = serde_json::from_slice(&decompressed)?;
        }
    }

    if compressed_count == 0 {
        anyhow::bail!("expected at least 1 compressed frame");
    }
    Ok(())
}

async fn test_client_streaming(
    sock: &TestSocket,
    encoding: &str,
    compress: CompressFn,
) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("connection error: {e}");
        }
    });

    let frame1 = envelope_frame(0x00, br#"{"message":"Hello from Alice"}"#);
    let compressed2 = compress(br#"{"message":"Hello from Bob via compressed frame"}"#)?;
    let frame2 = envelope_frame(0x01, &compressed2);
    let compressed3 = compress(br#"{"message":"Hello from Charlie also compressed"}"#)?;
    let frame3 = envelope_frame(0x01, &compressed3);
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
        .header("Connect-Content-Encoding", encoding)
        .header("Host", "localhost")
        .body(Full::new(Bytes::from(body)))?;

    let resp = sender.send_request(req).await?;
    let status = resp.status();
    if status != 200 {
        let body = resp.into_body().collect().await?.to_bytes();
        anyhow::bail!(
            "expected 200, got {}: {}",
            status,
            String::from_utf8_lossy(&body)
        );
    }

    let body_bytes = resp.into_body().collect().await?.to_bytes();
    let response_text = extract_message(&body_bytes)?;

    if !response_text.contains("3 messages") {
        anyhow::bail!("expected '3 messages', got: {response_text:?}");
    }
    for name in &["Alice", "Bob", "Charlie"] {
        if !response_text.contains(name) {
            anyhow::bail!("expected {name:?}, got: {response_text:?}");
        }
    }
    Ok(())
}

async fn test_unary_compression(
    sock: &TestSocket,
    encoding: &str,
    compress: CompressFn,
    decompress: DecompressFn,
) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("connection error: {e}");
        }
    });

    // Large request to trigger compression
    let large_name = format!("Test {} {}", encoding, "padding ".repeat(20));
    let json_body = format!(r#"{{"name":"{}"}}"#, large_name);
    let compressed_body = compress(json_body.as_bytes())?;

    let req = Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHello")
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .header("Content-Encoding", encoding)
        .header("Accept-Encoding", encoding)
        .header("Host", "localhost")
        .body(Full::new(Bytes::from(compressed_body)))?;

    let resp = sender.send_request(req).await?;
    let status = resp.status();
    if status != 200 {
        let body = resp.into_body().collect().await?.to_bytes();
        anyhow::bail!(
            "expected 200, got {}: {}",
            status,
            String::from_utf8_lossy(&body)
        );
    }

    let content_encoding = resp
        .headers()
        .get("content-encoding")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body_bytes = resp.into_body().collect().await?.to_bytes();

    let response_body = if content_encoding == encoding {
        decompress(&body_bytes)?
    } else {
        body_bytes.to_vec()
    };

    let json: serde_json::Value = serde_json::from_slice(&response_body)?;
    let message = json.get("message").and_then(|v| v.as_str()).unwrap_or("");
    if message.is_empty() {
        anyhow::bail!("expected non-empty message");
    }
    Ok(())
}

fn extract_message(body_bytes: &[u8]) -> anyhow::Result<String> {
    if body_bytes.len() >= 5 && (body_bytes[0] == 0x00 || body_bytes[0] == 0x01) {
        let mut cursor = &body_bytes[..];
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
                    return Ok(msg.to_string());
                }
            }
        }
        anyhow::bail!("no message frame found");
    } else {
        let json: serde_json::Value = serde_json::from_slice(body_bytes)?;
        Ok(json
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string())
    }
}

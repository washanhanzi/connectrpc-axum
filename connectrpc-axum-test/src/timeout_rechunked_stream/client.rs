use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use prost::Message;

use super::server;
use crate::socket::TestSocket;
use crate::{HelloRequest, HelloResponse};

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

pub async fn run_tests(sock: &TestSocket) -> Vec<CaseResult> {
    let cases: [(&'static str, Option<&'static str>); 2] = [
        // The client timeout arms ConnectLayer's deadline body wrapper, which
        // must track envelope boundaries across re-chunked body chunks.
        (
            "armed timeout delivers re-chunked stream intact",
            Some("10000"),
        ),
        // Control case: without a timeout the deadline wrapper is not applied.
        ("no timeout delivers re-chunked stream intact", None),
    ];

    let mut results = Vec::new();
    for (name, timeout_ms) in cases {
        let err = test_stream(sock, timeout_ms)
            .await
            .err()
            .map(|e| e.to_string());
        results.push(CaseResult { name, error: err });
    }
    results
}

async fn test_stream(sock: &TestSocket, timeout_ms: Option<&str>) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("connection error: {e}");
        }
    });

    let payload = HelloRequest {
        name: Some("Rechunk".to_string()),
        ..Default::default()
    }
    .encode_to_vec();
    let enveloped = envelope_frame(0x00, &payload);

    let mut builder = Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHelloStream")
        .header("Content-Type", "application/connect+proto")
        .header("Connect-Protocol-Version", "1")
        .header("Host", "localhost");
    if let Some(timeout_ms) = timeout_ms {
        builder = builder.header("Connect-Timeout-Ms", timeout_ms);
    }
    let req = builder.body(Full::new(Bytes::from(enveloped)))?;

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

    // Parse the envelope stream: [1 byte flags][4 bytes BE length][payload]
    let mut cursor = &body_bytes[..];
    let mut messages = Vec::new();
    let mut end_stream = None;
    while cursor.len() >= 5 {
        let flags = cursor[0];
        let len = u32::from_be_bytes([cursor[1], cursor[2], cursor[3], cursor[4]]) as usize;
        anyhow::ensure!(
            cursor.len() >= 5 + len,
            "truncated envelope after {} messages: have {} bytes, need {}",
            messages.len(),
            cursor.len(),
            5 + len
        );
        let payload = &cursor[5..5 + len];
        cursor = &cursor[5 + len..];
        if flags & 0x02 != 0 {
            end_stream = Some(payload.to_vec());
            break;
        }
        messages.push(HelloResponse::decode(payload)?);
    }

    let end_stream = end_stream.ok_or_else(|| {
        anyhow::anyhow!("missing EndStream frame after {} messages", messages.len())
    })?;
    anyhow::ensure!(
        cursor.is_empty(),
        "unexpected {} trailing bytes after EndStream",
        cursor.len()
    );
    let end_json: serde_json::Value = serde_json::from_slice(&end_stream)?;
    anyhow::ensure!(
        end_json.get("error").is_none(),
        "unexpected error in EndStream: {end_json}"
    );

    let expected = [
        server::decoy_message(),
        "second for Rechunk".to_string(),
        "third for Rechunk".to_string(),
    ];
    let got: Vec<&str> = messages.iter().map(|m| m.message.as_str()).collect();
    anyhow::ensure!(got == expected, "unexpected messages: {got:?}");
    Ok(())
}

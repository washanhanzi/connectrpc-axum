use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;
use connectrpc_axum_client::{ClientError, stream_interceptor};
use futures::stream;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::EchoRequest;
use crate::echo_service_connect_client::EchoServiceClient;
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

pub async fn run_client_stream_interceptor_tests(addr: SocketAddr) -> Vec<CaseResult> {
    let err = run_typed_send_interceptor_error_wins(addr)
        .await
        .err()
        .map(|e| e.to_string());
    vec![CaseResult {
        name: "typed on_send error takes precedence over call result",
        error: err,
    }]
}

/// A typed on_send interceptor failure must abort the request and surface the
/// interceptor's error to the caller, even though the aborted request body also
/// produces a transport/server error (regression test for generated clients).
async fn run_typed_send_interceptor_error_wins(addr: SocketAddr) -> anyhow::Result<()> {
    let sent = Arc::new(AtomicUsize::new(0));
    let client = EchoServiceClient::builder(format!("http://{addr}"))
        .http2_prior_knowledge()
        .with_on_send_echo_client_stream(stream_interceptor(move |_ctx, _msg: &mut EchoRequest| {
            if sent.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(())
            } else {
                Err(ClientError::invalid_argument("typed send blocked"))
            }
        }))
        .build()?;

    let messages = stream::iter(vec![
        EchoRequest {
            message: "first".to_string(),
        },
        EchoRequest {
            message: "second".to_string(),
        },
        EchoRequest {
            message: "third".to_string(),
        },
    ]);

    match client.echo_client_stream(messages).await {
        Err(e) if e.message() == Some("typed send blocked") => Ok(()),
        Err(e) => anyhow::bail!("expected typed send interceptor error, got: {e}"),
        Ok(resp) => anyhow::bail!(
            "expected typed send interceptor error, got success: {}",
            resp.into_inner().message
        ),
    }
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
    let response_text = if body_bytes.len() >= 5 && (body_bytes[0] == 0x00 || body_bytes[0] == 0x01)
    {
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

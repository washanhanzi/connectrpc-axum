use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use connectrpc_axum_client::{
    ClientError, ConnectClient, MessageInterceptor, StreamContext, stream_interceptor,
};
use futures::StreamExt;
use http::{Request, StatusCode, header};
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::Frame;
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use prost::Message;
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;

use crate::echo_service_connect_client::EchoServiceClient;
use crate::socket::TestSocket;
use crate::{EchoRequest, EchoResponse};

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

pub async fn run_bidi_stream_tests(sock: &TestSocket) -> Vec<CaseResult> {
    let (http1_result, http2_result) = tokio::join!(rejects_http1(sock), run_one_http2(sock));
    vec![
        CaseResult {
            name: "bidi stream rejects HTTP/1 without reading the request body",
            error: http1_result.err().map(|e| e.to_string()),
        },
        CaseResult {
            name: "bidi stream echoes messages over HTTP/2",
            error: http2_result.err().map(|e| e.to_string()),
        },
    ]
}

pub async fn run_bidi_stream_interceptor_tests(addr: SocketAddr) -> Vec<CaseResult> {
    let generic_err = run_send_interceptor_wakes_receive(addr)
        .await
        .err()
        .map(|e| e.to_string());
    let typed_err = run_typed_send_interceptor_wakes_receive(addr)
        .await
        .err()
        .map(|e| e.to_string());
    vec![
        CaseResult {
            name: "bidi send interceptor wakes pending receive",
            error: generic_err,
        },
        CaseResult {
            name: "bidi typed on_send error wakes pending receive",
            error: typed_err,
        },
    ]
}

#[derive(Clone, Default)]
struct FailSecondSend {
    sent: Arc<AtomicUsize>,
}

impl MessageInterceptor for FailSecondSend {
    fn on_stream_send<Req>(
        &self,
        _ctx: &StreamContext,
        _request: &mut Req,
    ) -> Result<(), ClientError>
    where
        Req: Message + Serialize + 'static,
    {
        if self.sent.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(())
        } else {
            Err(ClientError::invalid_argument("send blocked"))
        }
    }
}

async fn run_send_interceptor_wakes_receive(addr: SocketAddr) -> anyhow::Result<()> {
    let (request_tx, request_rx) = mpsc::channel(2);
    let (first_seen_tx, first_seen_rx) = oneshot::channel();

    let client = ConnectClient::builder(format!("http://{addr}"))
        .http2_prior_knowledge()
        .with_message_interceptor(FailSecondSend::default())
        .build()?;
    let receive = tokio::spawn(async move {
        let response = client
            .call_bidi_stream::<EchoRequest, EchoResponse, _>(
                "echo.EchoService/EchoBidiStream",
                ReceiverStream::new(request_rx),
            )
            .await?;
        let mut stream = response.into_inner();

        let first = tokio::time::timeout(Duration::from_secs(2), stream.next())
            .await
            .map_err(|_| anyhow::anyhow!("timed out waiting for first bidi response"))?
            .ok_or_else(|| anyhow::anyhow!("expected first bidi response, got stream end"))??;
        if !first.message.contains("Echo #1") {
            anyhow::bail!("expected first echo response, got {}", first.message);
        }

        let _ = first_seen_tx.send(());

        match tokio::time::timeout(Duration::from_secs(2), stream.next()).await {
            Ok(Some(Err(e))) if e.message() == Some("send blocked") => Ok(()),
            Ok(Some(Err(e))) => anyhow::bail!("expected send blocked error, got {e}"),
            Ok(Some(Ok(msg))) => anyhow::bail!("expected send error, got message {}", msg.message),
            Ok(None) => anyhow::bail!("expected send error, got stream end"),
            Err(_) => anyhow::bail!("pending receive was not woken by send interceptor error"),
        }
    });

    request_tx
        .send(EchoRequest {
            message: "first".to_string(),
        })
        .await
        .map_err(|_| anyhow::anyhow!("request stream closed before first send"))?;

    first_seen_rx
        .await
        .map_err(|_| anyhow::anyhow!("receive task ended before first response"))?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    request_tx
        .send(EchoRequest {
            message: "second".to_string(),
        })
        .await
        .map_err(|_| anyhow::anyhow!("request stream closed before second send"))?;

    receive.await?
}

/// Same as [`run_send_interceptor_wakes_receive`], but through the generated
/// typed client's `on_send` interceptor (regression test: the typed error must
/// win over the transport error caused by the aborted request body).
async fn run_typed_send_interceptor_wakes_receive(addr: SocketAddr) -> anyhow::Result<()> {
    let (request_tx, request_rx) = mpsc::channel(2);
    let (first_seen_tx, first_seen_rx) = oneshot::channel();

    let sent = Arc::new(AtomicUsize::new(0));
    let client = EchoServiceClient::builder(format!("http://{addr}"))
        .http2_prior_knowledge()
        .with_on_send_echo_bidi_stream(stream_interceptor(move |ctx, _msg: &mut EchoRequest| {
            if ctx
                .request_headers
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                != Some("application/connect+json")
                || ctx
                    .request_headers
                    .get("connect-protocol-version")
                    .and_then(|value| value.to_str().ok())
                    != Some("1")
            {
                return Err(ClientError::internal(
                    "typed send interceptor received incomplete request headers",
                ));
            }

            if sent.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(())
            } else {
                Err(ClientError::invalid_argument("typed send blocked"))
            }
        }))
        .build()?;
    let receive = tokio::spawn(async move {
        let response = client
            .echo_bidi_stream(ReceiverStream::new(request_rx))
            .await?;
        let mut stream = response.into_inner();

        let first = tokio::time::timeout(Duration::from_secs(2), stream.next())
            .await
            .map_err(|_| anyhow::anyhow!("timed out waiting for first bidi response"))?
            .ok_or_else(|| anyhow::anyhow!("expected first bidi response, got stream end"))??;
        if !first.message.contains("Echo #1") {
            anyhow::bail!("expected first echo response, got {}", first.message);
        }

        let _ = first_seen_tx.send(());

        match tokio::time::timeout(Duration::from_secs(2), stream.next()).await {
            Ok(Some(Err(e))) if e.message() == Some("typed send blocked") => Ok(()),
            Ok(Some(Err(e))) => anyhow::bail!("expected typed send blocked error, got {e}"),
            Ok(Some(Ok(msg))) => anyhow::bail!("expected send error, got message {}", msg.message),
            Ok(None) => anyhow::bail!("expected send error, got stream end"),
            Err(_) => {
                anyhow::bail!("pending receive was not woken by typed send interceptor error")
            }
        }
    });

    request_tx
        .send(EchoRequest {
            message: "first".to_string(),
        })
        .await
        .map_err(|_| anyhow::anyhow!("request stream closed before first send"))?;

    first_seen_rx
        .await
        .map_err(|_| anyhow::anyhow!("receive task ended before first response"))?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    request_tx
        .send(EchoRequest {
            message: "second".to_string(),
        })
        .await
        .map_err(|_| anyhow::anyhow!("request stream closed before second send"))?;

    receive.await?
}

async fn rejects_http1(sock: &TestSocket) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let first_frame = envelope_frame(0x00, br#"{"message":"Hello"}"#);
    let request_frames =
        futures::stream::once(
            async move { Ok::<_, Infallible>(Frame::data(Bytes::from(first_frame))) },
        )
        .chain(futures::stream::pending());

    let req = Request::builder()
        .method("POST")
        .uri("/echo.EchoService/EchoBidiStream")
        .header("Content-Type", "application/connect+json")
        .header("Connect-Protocol-Version", "1")
        .header("Host", "localhost")
        .body(StreamBody::new(request_frames))?;

    let resp = tokio::time::timeout(Duration::from_secs(1), sender.send_request(req))
        .await
        .map_err(|_| anyhow::anyhow!("HTTP/1 bidi request was not rejected promptly"))??;

    if resp.status() != StatusCode::HTTP_VERSION_NOT_SUPPORTED {
        anyhow::bail!(
            "expected HTTP 505 for HTTP/1 bidi request, got {}",
            resp.status()
        );
    }
    if resp.headers().get(header::CONNECTION) != Some(&http::HeaderValue::from_static("close")) {
        anyhow::bail!(
            "expected Connection: close for HTTP/1 bidi request, got {:?}",
            resp.headers().get(header::CONNECTION)
        );
    }

    let response_body = tokio::time::timeout(Duration::from_secs(1), resp.into_body().collect())
        .await
        .map_err(|_| anyhow::anyhow!("HTTP/1 rejection body did not finish promptly"))??
        .to_bytes();
    if !response_body.is_empty() {
        anyhow::bail!(
            "expected empty HTTP/1 rejection body, got {} bytes",
            response_body.len()
        );
    }

    Ok(())
}

/// Bidi stream over HTTP/2 (required by connect-go servers)
async fn run_one_http2(sock: &TestSocket) -> anyhow::Result<()> {
    let (mut sender, _handle) = crate::socket::http2_connect(sock).await?;

    let body = build_bidi_request_body();

    let req = Request::builder()
        .method("POST")
        .uri("http://localhost/echo.EchoService/EchoBidiStream")
        .header("Content-Type", "application/connect+json")
        .header("Connect-Protocol-Version", "1")
        .body(Full::new(Bytes::from(body)))?;

    let resp = sender.send_request(req).await?;
    validate_response(resp).await
}

fn build_bidi_request_body() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&envelope_frame(0x00, br#"{"message":"Hello"}"#));
    body.extend_from_slice(&envelope_frame(0x00, br#"{"message":"World"}"#));
    body.extend_from_slice(&envelope_frame(0x00, br#"{"message":"Bidi"}"#));
    body.extend_from_slice(&envelope_frame(0x02, b"{}"));
    body
}

async fn validate_response(resp: http::Response<hyper::body::Incoming>) -> anyhow::Result<()> {
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if !content_type.starts_with("application/connect+json") {
        let body_bytes = resp.into_body().collect().await?.to_bytes();
        anyhow::bail!(
            "expected content-type application/connect+json, got: {content_type} (body: {})",
            String::from_utf8_lossy(&body_bytes)
        );
    }

    let body_bytes = resp.into_body().collect().await?.to_bytes();
    let mut cursor = &body_bytes[..];
    let mut messages = Vec::new();

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

        let json: serde_json::Value = serde_json::from_slice(payload)?;
        messages.push(json);
    }

    if messages.len() < 3 {
        anyhow::bail!("expected at least 3 messages, got {}", messages.len());
    }

    let first_message = messages[0]
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "expected message field in first frame, got: {}",
                messages[0]
            )
        })?;

    if !first_message.contains("Echo #1") {
        anyhow::bail!(
            "expected first message to contain 'Echo #1', got: {:?}",
            first_message
        );
    }

    Ok(())
}

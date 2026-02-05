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

pub async fn run_idempotency_get_tests(sock: &TestSocket) -> Vec<CaseResult> {
    let err = test_get_greeting(sock).await.err().map(|e| e.to_string());
    vec![CaseResult {
        name: "GET request for idempotent method works",
        error: err,
    }]
}

async fn test_get_greeting(sock: &TestSocket) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    // Send GET request with query parameters (Connect GET format):
    // /hello.HelloWorldService/GetGreeting?encoding=json&connect=v1&message=%7B%22name%22%3A%22GetUser%22%7D
    let req = Request::builder()
        .method("GET")
        .uri("/hello.HelloWorldService/GetGreeting?encoding=json&connect=v1&message=%7B%22name%22%3A%22GetUser%22%7D")
        .header("Host", "localhost")
        .body(Full::new(Bytes::new()))?;

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

    let body = resp.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)?;
    let message = json.get("message").and_then(|v| v.as_str()).unwrap_or("");
    if !message.contains("GetUser") {
        anyhow::bail!("expected message to contain 'GetUser', got: {message:?}");
    }
    Ok(())
}

use std::path::{Path, PathBuf};
use std::process::Stdio;

use buffa::Message;
use bytes::Bytes;
use connectrpc_axum::prelude::*;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use tokio::process::Command;

use crate::{HelloRequest, HelloResponse, hello_world_service_connect, socket::TestSocket};

async fn say_hello(
    req: ViewRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    let req = req.0;
    let name = req.name.unwrap_or("World");
    Ok(ConnectResponse::new(HelloResponse {
        message: format!("Hello, {name}!"),
        response_type: None,
        ..Default::default()
    }))
}

async fn start(listener: tokio::net::UnixListener) -> anyhow::Result<()> {
    let router = hello_world_service_connect::HelloWorldServiceBuilder::new()
        .say_hello(say_hello)
        .build();

    let app = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(router)
        .build();

    axum::serve(listener, app).await?;
    Ok(())
}

pub async fn run(rust_sock: &TestSocket, _go_sock: &TestSocket) -> anyhow::Result<()> {
    let go_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("go");
    let go_client_bin =
        build_go_binary(&go_dir, "./view_unary/client/", "view-unary-client").await?;

    let rust_listener = rust_sock.bind()?;
    let rust_server = tokio::spawn(start(rust_listener));

    rust_sock.wait_ready().await?;

    run_proto_request(rust_sock).await?;
    run_json_request(rust_sock).await?;
    let result = run_go_client(rust_sock, &go_client_bin).await;

    rust_server.abort();

    result
}

async fn run_proto_request(sock: &TestSocket) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let body = HelloRequest {
        name: Some("Proto View".to_string()),
        ..Default::default()
    }
    .encode_to_vec();

    let req = Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHello")
        .header("Content-Type", "application/proto")
        .header("Connect-Protocol-Version", "1")
        .header("Host", "localhost")
        .body(Full::new(Bytes::from(body)))?;

    let resp = sender.send_request(req).await?;
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !content_type.starts_with("application/proto") {
        anyhow::bail!("expected content-type application/proto, got: {content_type}");
    }

    let resp_body = resp.into_body().collect().await?.to_bytes();
    let response = HelloResponse::decode_from_slice(&resp_body)?;
    if response.message != "Hello, Proto View!" {
        anyhow::bail!("unexpected response: {:?}", response.message);
    }

    Ok(())
}

async fn run_json_request(sock: &TestSocket) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let req = Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHello")
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .header("Host", "localhost")
        .body(Full::new(Bytes::from_static(br#"{"name":"JSON View"}"#)))?;

    let resp = sender.send_request(req).await?;
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !content_type.starts_with("application/json") {
        anyhow::bail!("expected content-type application/json, got: {content_type}");
    }

    let resp_body = resp.into_body().collect().await?.to_bytes();
    let response: HelloResponse = serde_json::from_slice(&resp_body)?;
    if response.message != "Hello, JSON View!" {
        anyhow::bail!("unexpected response: {:?}", response.message);
    }

    Ok(())
}

async fn build_go_binary(go_dir: &Path, pkg: &str, name: &str) -> anyhow::Result<PathBuf> {
    let bin_dir = go_dir.join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    let out = bin_dir.join(name);
    let status = Command::new("go")
        .args(["build", "-o", out.to_str().unwrap(), pkg])
        .current_dir(go_dir)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await?;
    if !status.success() {
        anyhow::bail!("go build {pkg} failed");
    }
    Ok(out)
}

async fn run_go_client(sock: &TestSocket, go_client_bin: &Path) -> anyhow::Result<()> {
    let status = Command::new(go_client_bin)
        .env("SOCKET_PATH", sock.go_addr())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await?;
    if !status.success() {
        anyhow::bail!("Go client tests failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn view_request_accepts_proto_unary() {
        let socket = TestSocket::new(&format!("connectrpc-view-unary-{}", std::process::id()))
            .expect("socket");
        let listener = socket.bind().expect("bind");
        let server = tokio::spawn(start(listener));

        socket.wait_ready().await.expect("server ready");
        run_proto_request(&socket)
            .await
            .expect("proto request succeeds");

        server.abort();
    }

    #[tokio::test]
    async fn view_request_accepts_json_unary() {
        let socket = TestSocket::new(&format!(
            "connectrpc-view-unary-json-{}",
            std::process::id()
        ))
        .expect("socket");
        let listener = socket.bind().expect("bind");
        let server = tokio::spawn(start(listener));

        socket.wait_ready().await.expect("server ready");
        run_json_request(&socket)
            .await
            .expect("json request succeeds");

        server.abort();
    }

    #[tokio::test]
    async fn view_request_accepts_go_client() {
        let go_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("go");
        let go_client_bin = build_go_binary(&go_dir, "./view_unary/client/", "view-unary-client")
            .await
            .expect("go client builds");

        let socket = TestSocket::new(&format!("connectrpc-view-unary-go-{}", std::process::id()))
            .expect("socket");
        let listener = socket.bind().expect("bind");
        let server = tokio::spawn(start(listener));

        socket.wait_ready().await.expect("server ready");
        run_go_client(&socket, &go_client_bin)
            .await
            .expect("go client succeeds");

        server.abort();
    }
}

use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;

use crate::socket::TestSocket;

enum TestMethod {
    Post {
        content_type: &'static str,
    },
    Get {
        uri: &'static str,
    },
}

struct TestCase {
    name: &'static str,
    method: TestMethod,
    expect_status: u16,
    expect_empty_body: bool,
    expect_accept_post: bool,
}

const GET_URI_MSGPACK: &str = "/hello.HelloWorldService/GetGreeting?encoding=msgpack&connect=v1&message=eyJuYW1lIjoiQWxpY2UifQ";

static TEST_CASES: &[TestCase] = &[
    TestCase {
        name: "POST with Content-Type: text/plain returns 415",
        method: TestMethod::Post {
            content_type: "text/plain",
        },
        expect_status: 415,
        expect_empty_body: true,
        expect_accept_post: true,
    },
    TestCase {
        name: "POST with Content-Type: application/xml returns 415",
        method: TestMethod::Post {
            content_type: "application/xml",
        },
        expect_status: 415,
        expect_empty_body: true,
        expect_accept_post: true,
    },
    TestCase {
        name: "GET with unsupported encoding=msgpack returns 415",
        method: TestMethod::Get {
            uri: GET_URI_MSGPACK,
        },
        expect_status: 415,
        expect_empty_body: true,
        expect_accept_post: true,
    },
];

pub struct CaseResult {
    pub name: &'static str,
    pub error: Option<String>,
}

pub async fn run_protocol_negotiation_tests(sock: &TestSocket) -> Vec<CaseResult> {
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

async fn run_one(sock: &TestSocket, tc: &TestCase) -> anyhow::Result<()> {
    let stream = sock.connect().await?;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("connection error: {e}");
        }
    });

    let req = match &tc.method {
        TestMethod::Post { content_type } => {
            Request::builder()
                .method("POST")
                .uri("/hello.HelloWorldService/SayHello")
                .header("Content-Type", *content_type)
                .header("Host", "localhost")
                .body(Full::new(Bytes::from(r#"{"name":"test"}"#)))?
        }
        TestMethod::Get { uri } => {
            Request::builder()
                .method("GET")
                .uri(*uri)
                .header("Host", "localhost")
                .body(Full::new(Bytes::new()))?
        }
    };

    let resp = sender.send_request(req).await?;
    let status = resp.status().as_u16();
    let accept_post = resp
        .headers()
        .get("Accept-Post")
        .map(|v| v.to_str().unwrap_or("").to_string());
    let resp_body = resp.into_body().collect().await?.to_bytes();

    // Check status code
    if status != tc.expect_status {
        anyhow::bail!(
            "expected HTTP {}, got {status}. Body: {:?}",
            tc.expect_status,
            String::from_utf8_lossy(&resp_body)
        );
    }

    // Check empty body for 415 responses
    if tc.expect_empty_body && !resp_body.is_empty() {
        anyhow::bail!(
            "expected empty body for HTTP {}, got {:?} (len={})",
            tc.expect_status,
            String::from_utf8_lossy(&resp_body),
            resp_body.len()
        );
    }

    // Check Accept-Post header
    if tc.expect_accept_post {
        match &accept_post {
            Some(val) if !val.is_empty() => {
                // Verify it contains expected content types
                if !val.contains("application/json") && !val.contains("application/connect+json") {
                    anyhow::bail!(
                        "Accept-Post header {:?} doesn't contain expected content types",
                        val
                    );
                }
            }
            _ => {
                anyhow::bail!("expected Accept-Post header, but not present");
            }
        }
    }

    Ok(())
}

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Context;
use axum::serve::ListenerExt;
use bytes::Bytes;
use connectrpc_axum::prelude::*;
use connectrpc_axum_examples::{HelloRequest, HelloResponse, hello_world_service_connect};
use http::{Request, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http2;
use hyper_util::rt::{TokioExecutor, TokioIo};

const PAYLOAD: &str = "perf-smoke";
const MAX_LATENCY_SAMPLES: usize = 250_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ServerMode {
    Axum,
    TapIoNoop,
    TapIoNodelay,
}

impl ServerMode {
    fn label(self) -> &'static str {
        match self {
            Self::Axum => "axum::serve",
            Self::TapIoNoop => "axum::serve + tap_io",
            Self::TapIoNodelay => "axum::serve + nodelay",
        }
    }
}

struct BenchResult {
    mode: ServerMode,
    concurrency: usize,
    rps: f64,
    p50_us: u64,
    p99_us: u64,
}

async fn say_hello(
    ConnectRequest(req): ConnectRequest<HelloRequest>,
) -> Result<ConnectResponse<HelloResponse>, ConnectError> {
    Ok(ConnectResponse::new(HelloResponse {
        message: req.name.unwrap_or_else(|| "world".to_string()),
        response_type: None,
    }))
}

fn grpc_frame(name: &str) -> Bytes {
    let name = name.as_bytes();
    let mut proto = Vec::with_capacity(name.len() + 2);
    proto.push(0x0a);
    proto.push(name.len() as u8);
    proto.extend_from_slice(name);

    let mut frame = Vec::with_capacity(proto.len() + 5);
    frame.push(0x00);
    frame.extend_from_slice(&(proto.len() as u32).to_be_bytes());
    frame.extend_from_slice(&proto);
    Bytes::from(frame)
}

async fn start_server(
    mode: ServerMode,
) -> anyhow::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
    let (connect_router, grpc_server) =
        hello_world_service_connect::HelloWorldServiceTonicCompatibleBuilder::new()
            .say_hello(say_hello)
            .build();

    let service = connectrpc_axum::MakeServiceBuilder::new()
        .add_router(connect_router)
        .add_grpc_service(grpc_server)
        .build();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let handle = tokio::spawn(async move {
        match mode {
            ServerMode::Axum => {
                let _ = axum::serve(listener, tower::make::Shared::new(service)).await;
            }
            ServerMode::TapIoNoop => {
                let listener = listener.tap_io(|_tcp_stream| {});
                let _ = axum::serve(listener, tower::make::Shared::new(service)).await;
            }
            ServerMode::TapIoNodelay => {
                let listener = listener.tap_io(|tcp_stream| {
                    let _ = tcp_stream.set_nodelay(true);
                });
                let _ = axum::serve(listener, tower::make::Shared::new(service)).await;
            }
        }
    });

    Ok((addr, handle))
}

async fn open_connection(
    addr: SocketAddr,
) -> anyhow::Result<hyper::client::conn::http2::SendRequest<Full<Bytes>>> {
    let stream = tokio::net::TcpStream::connect(addr).await?;
    let io = TokioIo::new(stream);
    let (sender, conn) = http2::Builder::new(TokioExecutor::new())
        .handshake(io)
        .await
        .context("http2 handshake failed")?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    Ok(sender)
}

async fn run_request(
    mut sender: hyper::client::conn::http2::SendRequest<Full<Bytes>>,
    frame: Bytes,
) -> anyhow::Result<()> {
    sender.ready().await?;
    let req = Request::builder()
        .method("POST")
        .uri("http://localhost/hello.HelloWorldService/SayHello")
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .body(Full::new(frame))?;

    let resp = sender.send_request(req).await?;
    let status = resp.status();
    if status != StatusCode::OK {
        anyhow::bail!("unexpected HTTP status {status}");
    }

    let headers = resp.headers().clone();
    let trailers = resp
        .into_body()
        .collect()
        .await
        .context("read grpc response body")?
        .trailers()
        .cloned()
        .unwrap_or_default();

    let grpc_status = trailers
        .get("grpc-status")
        .or_else(|| headers.get("grpc-status"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("0");
    if grpc_status != "0" {
        anyhow::bail!("unexpected grpc-status {grpc_status}");
    }

    Ok(())
}

async fn bench_mode(
    mode: ServerMode,
    concurrency: usize,
    warmup: Duration,
    measurement: Duration,
) -> anyhow::Result<BenchResult> {
    let (addr, handle) = start_server(mode).await?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let sender: hyper::client::conn::http2::SendRequest<Full<Bytes>> =
        open_connection(addr).await?;
    let frame = grpc_frame(PAYLOAD);
    let running = Arc::new(AtomicBool::new(true));
    let completed = Arc::new(AtomicU64::new(0));
    let latencies = Arc::new(tokio::sync::Mutex::new(Vec::with_capacity(
        MAX_LATENCY_SAMPLES.min(measurement.as_secs() as usize * 50_000),
    )));

    let mut workers = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let sender = sender.clone();
        let frame = frame.clone();
        let running = Arc::clone(&running);
        let completed = Arc::clone(&completed);
        let latencies = Arc::clone(&latencies);
        workers.push(tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                let start = Instant::now();
                if run_request(sender.clone(), frame.clone()).await.is_ok() {
                    let elapsed = start.elapsed();
                    let n = completed.fetch_add(1, Ordering::Relaxed);
                    if n.is_multiple_of(10) {
                        let mut samples = latencies.lock().await;
                        if samples.len() < MAX_LATENCY_SAMPLES {
                            samples.push(elapsed.as_micros() as u64);
                        }
                    }
                }
            }
        }));
    }

    tokio::time::sleep(warmup).await;
    completed.store(0, Ordering::Relaxed);
    latencies.lock().await.clear();
    let measure_start = Instant::now();

    tokio::time::sleep(measurement).await;
    running.store(false, Ordering::Relaxed);
    let elapsed = measure_start.elapsed();

    for worker in workers {
        let _ = worker.await;
    }

    handle.abort();

    let total = completed.load(Ordering::Relaxed);
    let rps = total as f64 / elapsed.as_secs_f64();
    let mut samples = latencies.lock().await;
    samples.sort_unstable();
    let p50 = if samples.is_empty() {
        0
    } else {
        samples[samples.len() / 2]
    };
    let p99 = if samples.is_empty() {
        0
    } else {
        samples[samples.len() * 99 / 100]
    };

    Ok(BenchResult {
        mode,
        concurrency,
        rps,
        p50_us: p50,
        p99_us: p99,
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let concurrency = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(16usize);
    let warmup_ms = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1_000u64);
    let measure_ms = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(3_000u64);
    let warmup = Duration::from_millis(warmup_ms);
    let measurement = Duration::from_millis(measure_ms);

    let axum = bench_mode(ServerMode::Axum, concurrency, warmup, measurement).await?;
    let tap_noop = bench_mode(ServerMode::TapIoNoop, concurrency, warmup, measurement).await?;
    let tap_nodelay =
        bench_mode(ServerMode::TapIoNodelay, concurrency, warmup, measurement).await?;

    println!(
        "{:<22} {:>12} {:>14} {:>12} {:>12}",
        "Mode", "Concurrency", "Requests/sec", "p50 (ms)", "p99 (ms)"
    );
    println!("{}", "-".repeat(78));
    for result in [axum, tap_noop, tap_nodelay] {
        println!(
            "{:<22} {:>12} {:>14.0} {:>12.2} {:>12.2}",
            result.mode.label(),
            result.concurrency,
            result.rps,
            result.p50_us as f64 / 1000.0,
            result.p99_us as f64 / 1000.0,
        );
    }

    Ok(())
}

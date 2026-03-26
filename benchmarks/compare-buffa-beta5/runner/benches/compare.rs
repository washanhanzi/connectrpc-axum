use axum::Router;
use bytes::Bytes;
use compare_buffa_beta5_cases_buffa as buffa;
use compare_buffa_beta5_cases_connectrpc as connectrust;
use compare_buffa_beta5_cases_release as release;
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use http::{Request, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper_util::client::legacy::{Client, connect::HttpConnector};
use hyper_util::rt::TokioExecutor;
use std::fs::File;
use std::os::fd::AsRawFd;
use tokio::runtime::Runtime;
use tokio::sync::oneshot;
use tokio::time::{Duration, sleep};

struct StderrSilencer {
    original_fd: i32,
}

impl StderrSilencer {
    fn new() -> Self {
        let dev_null = File::options()
            .write(true)
            .open("/dev/null")
            .expect("open /dev/null");

        let original_fd = unsafe { libc::dup(libc::STDERR_FILENO) };
        assert!(original_fd >= 0, "dup stderr");

        let redirect_result = unsafe { libc::dup2(dev_null.as_raw_fd(), libc::STDERR_FILENO) };
        assert!(redirect_result >= 0, "redirect stderr");

        Self { original_fd }
    }
}

impl Drop for StderrSilencer {
    fn drop(&mut self) {
        unsafe {
            libc::dup2(self.original_fd, libc::STDERR_FILENO);
            libc::close(self.original_fd);
        }
    }
}

type HttpClient = Client<HttpConnector, Full<Bytes>>;

const UNARY_PATH: &str = "/hello.HelloWorldService/SayHello";
const STREAM_PATH: &str = "/hello.HelloWorldService/SayHelloStream";

struct BenchmarkServer {
    base_url: String,
    shutdown: Option<oneshot::Sender<()>>,
}

impl Drop for BenchmarkServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

fn http_client() -> HttpClient {
    let connector = HttpConnector::new();
    Client::builder(TokioExecutor::new()).build(connector)
}

async fn spawn_server(app: Router) -> BenchmarkServer {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind benchmark server");
    let addr = listener.local_addr().expect("server local addr");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("run benchmark server");
    });

    sleep(Duration::from_millis(10)).await;

    BenchmarkServer {
        base_url: format!("http://{addr}"),
        shutdown: Some(shutdown_tx),
    }
}

fn connect_request(
    base_url: &str,
    path: &str,
    content_type: &str,
    body: Vec<u8>,
) -> Request<Full<Bytes>> {
    Request::builder()
        .method("POST")
        .uri(format!("{base_url}{path}"))
        .header("Content-Type", content_type)
        .header("Connect-Protocol-Version", "1")
        .body(Full::new(Bytes::from(body)))
        .expect("build benchmark request")
}

async fn send_request(client: &HttpClient, request: Request<Full<Bytes>>) -> Bytes {
    let response = client.request(request).await.expect("client response");
    assert_eq!(response.status(), StatusCode::OK);
    response
        .into_body()
        .collect()
        .await
        .expect("collect response body")
        .to_bytes()
}

fn connect_unary_proto_roundtrip_benchmarks(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let buffa_server = rt.block_on(spawn_server(buffa::connect_app()));
    let release_server = rt.block_on(spawn_server(release::connect_app()));
    let connectrust_server = rt.block_on(spawn_server(connectrust::connect_app()));
    let client = http_client();

    let mut group = c.benchmark_group("connect_unary_proto_roundtrip");
    group.sample_size(10);

    for size in [
        buffa::PayloadSize::Small,
        buffa::PayloadSize::Medium,
        buffa::PayloadSize::Large,
    ] {
        let request = buffa::hello_request(size);
        let proto_body = buffa::encode_hello_request_proto(&request);

        group.throughput(Throughput::Bytes(proto_body.len() as u64));

        group.bench_function(BenchmarkId::new("buffa", size.as_str()), |b| {
            b.to_async(&rt).iter(|| {
                let client = client.clone();
                let base_url = buffa_server.base_url.clone();
                let body = proto_body.clone();
                async move {
                    let bytes = send_request(
                        &client,
                        connect_request(&base_url, UNARY_PATH, "application/proto", body),
                    )
                    .await;
                    black_box(buffa::decode_hello_response_proto(&bytes));
                }
            });
        });

        group.bench_function(BenchmarkId::new("v0.1.0", size.as_str()), |b| {
            b.to_async(&rt).iter(|| {
                let client = client.clone();
                let base_url = release_server.base_url.clone();
                let body = proto_body.clone();
                async move {
                    let bytes = send_request(
                        &client,
                        connect_request(&base_url, UNARY_PATH, "application/proto", body),
                    )
                    .await;
                    black_box(buffa::decode_hello_response_proto(&bytes));
                }
            });
        });

        group.bench_function(BenchmarkId::new("connect-rust", size.as_str()), |b| {
            b.to_async(&rt).iter(|| {
                let client = client.clone();
                let base_url = connectrust_server.base_url.clone();
                let body = proto_body.clone();
                async move {
                    let bytes = send_request(
                        &client,
                        connect_request(&base_url, UNARY_PATH, "application/proto", body),
                    )
                    .await;
                    black_box(buffa::decode_hello_response_proto(&bytes));
                }
            });
        });
    }

    group.finish();
}

fn connect_unary_json_roundtrip_benchmarks(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let buffa_server = rt.block_on(spawn_server(buffa::connect_app()));
    let release_server = rt.block_on(spawn_server(release::connect_app()));
    let connectrust_server = rt.block_on(spawn_server(connectrust::connect_app()));
    let client = http_client();

    let mut group = c.benchmark_group("connect_unary_json_roundtrip");
    group.sample_size(10);

    for size in [
        buffa::PayloadSize::Small,
        buffa::PayloadSize::Medium,
        buffa::PayloadSize::Large,
    ] {
        let request = buffa::hello_request(size);
        let json_body = buffa::encode_hello_request_json(&request);

        group.throughput(Throughput::Bytes(json_body.len() as u64));

        group.bench_function(BenchmarkId::new("buffa", size.as_str()), |b| {
            b.to_async(&rt).iter(|| {
                let client = client.clone();
                let base_url = buffa_server.base_url.clone();
                let body = json_body.clone();
                async move {
                    let bytes = send_request(
                        &client,
                        connect_request(&base_url, UNARY_PATH, "application/json", body),
                    )
                    .await;
                    black_box(buffa::decode_hello_response_json(&bytes));
                }
            });
        });

        group.bench_function(BenchmarkId::new("v0.1.0", size.as_str()), |b| {
            b.to_async(&rt).iter(|| {
                let client = client.clone();
                let base_url = release_server.base_url.clone();
                let body = json_body.clone();
                async move {
                    let bytes = send_request(
                        &client,
                        connect_request(&base_url, UNARY_PATH, "application/json", body),
                    )
                    .await;
                    black_box(buffa::decode_hello_response_json(&bytes));
                }
            });
        });

        group.bench_function(BenchmarkId::new("connect-rust", size.as_str()), |b| {
            b.to_async(&rt).iter(|| {
                let client = client.clone();
                let base_url = connectrust_server.base_url.clone();
                let body = json_body.clone();
                async move {
                    let bytes = send_request(
                        &client,
                        connect_request(&base_url, UNARY_PATH, "application/json", body),
                    )
                    .await;
                    black_box(buffa::decode_hello_response_json(&bytes));
                }
            });
        });
    }

    group.finish();
}

fn connect_stream_proto_roundtrip_benchmarks(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let buffa_server = rt.block_on(spawn_server(buffa::connect_app()));
    let release_server = rt.block_on(spawn_server(release::connect_app()));
    let connectrust_server = rt.block_on(spawn_server(connectrust::connect_app()));
    let client = http_client();

    let mut group = c.benchmark_group("connect_stream_proto_roundtrip");
    group.sample_size(10);
    let _stderr_silencer = StderrSilencer::new();

    for size in [
        buffa::PayloadSize::Small,
        buffa::PayloadSize::Medium,
        buffa::PayloadSize::Large,
    ] {
        let request = buffa::hello_request(size);
        let proto_body = buffa::envelope_frame(&buffa::encode_hello_request_proto(&request));

        group.throughput(Throughput::Bytes(proto_body.len() as u64));

        group.bench_function(BenchmarkId::new("buffa", size.as_str()), |b| {
            b.to_async(&rt).iter(|| {
                let client = client.clone();
                let base_url = buffa_server.base_url.clone();
                let body = proto_body.clone();
                async move {
                    let bytes = send_request(
                        &client,
                        connect_request(&base_url, STREAM_PATH, "application/connect+proto", body),
                    )
                    .await;
                    black_box(buffa::parse_streaming_proto_responses(&bytes));
                }
            });
        });

        group.bench_function(BenchmarkId::new("v0.1.0", size.as_str()), |b| {
            b.to_async(&rt).iter(|| {
                let client = client.clone();
                let base_url = release_server.base_url.clone();
                let body = proto_body.clone();
                async move {
                    let bytes = send_request(
                        &client,
                        connect_request(&base_url, STREAM_PATH, "application/connect+proto", body),
                    )
                    .await;
                    black_box(buffa::parse_streaming_proto_responses(&bytes));
                }
            });
        });

        group.bench_function(BenchmarkId::new("connect-rust", size.as_str()), |b| {
            b.to_async(&rt).iter(|| {
                let client = client.clone();
                let base_url = connectrust_server.base_url.clone();
                let body = proto_body.clone();
                async move {
                    let bytes = send_request(
                        &client,
                        connect_request(&base_url, STREAM_PATH, "application/connect+proto", body),
                    )
                    .await;
                    black_box(buffa::parse_streaming_proto_responses(&bytes));
                }
            });
        });
    }

    group.finish();
}

fn benchmark_configuration() -> Criterion {
    Criterion::default().configure_from_args()
}

criterion_group! {
    name = benches;
    config = benchmark_configuration();
    targets =
        connect_unary_proto_roundtrip_benchmarks,
        connect_unary_json_roundtrip_benchmarks,
        connect_stream_proto_roundtrip_benchmarks
}
criterion_main!(benches);

use compare_buffa_beta5_cases_beta5 as beta5;
use compare_buffa_beta5_cases_buffa as buffa;
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use http::StatusCode;
use http_body_util::BodyExt;
use std::fs::File;
use std::os::fd::AsRawFd;
use tokio::runtime::Runtime;
use tower::ServiceExt;

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

fn proto_encode_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("proto_encode_hello_request");

    for size in [
        buffa::PayloadSize::Small,
        buffa::PayloadSize::Medium,
        buffa::PayloadSize::Large,
    ] {
        let buffa_request = buffa::hello_request(size);
        let beta5_request = beta5::hello_request(match size {
            buffa::PayloadSize::Small => beta5::PayloadSize::Small,
            buffa::PayloadSize::Medium => beta5::PayloadSize::Medium,
            buffa::PayloadSize::Large => beta5::PayloadSize::Large,
        });

        group.throughput(Throughput::Bytes(
            buffa::encode_hello_request_proto(&buffa_request).len() as u64,
        ));

        group.bench_function(BenchmarkId::new("buffa", size.as_str()), |b| {
            b.iter(|| black_box(buffa::encode_hello_request_proto(black_box(&buffa_request))))
        });

        group.bench_function(BenchmarkId::new("beta5", size.as_str()), |b| {
            b.iter(|| black_box(beta5::encode_hello_request_proto(black_box(&beta5_request))))
        });
    }

    group.finish();
}

fn proto_decode_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("proto_decode_hello_request");

    for size in [
        buffa::PayloadSize::Small,
        buffa::PayloadSize::Medium,
        buffa::PayloadSize::Large,
    ] {
        let buffa_bytes = buffa::encode_hello_request_proto(&buffa::hello_request(size));
        let beta5_bytes = beta5::encode_hello_request_proto(&beta5::hello_request(match size {
            buffa::PayloadSize::Small => beta5::PayloadSize::Small,
            buffa::PayloadSize::Medium => beta5::PayloadSize::Medium,
            buffa::PayloadSize::Large => beta5::PayloadSize::Large,
        }));

        group.throughput(Throughput::Bytes(buffa_bytes.len() as u64));

        group.bench_function(BenchmarkId::new("buffa", size.as_str()), |b| {
            b.iter(|| black_box(buffa::decode_hello_request_proto(black_box(&buffa_bytes))))
        });

        group.bench_function(BenchmarkId::new("beta5", size.as_str()), |b| {
            b.iter(|| black_box(beta5::decode_hello_request_proto(black_box(&beta5_bytes))))
        });
    }

    group.finish();
}

fn json_encode_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_encode_hello_request");

    for size in [
        buffa::PayloadSize::Small,
        buffa::PayloadSize::Medium,
        buffa::PayloadSize::Large,
    ] {
        let buffa_request = buffa::hello_request(size);
        let beta5_request = beta5::hello_request(match size {
            buffa::PayloadSize::Small => beta5::PayloadSize::Small,
            buffa::PayloadSize::Medium => beta5::PayloadSize::Medium,
            buffa::PayloadSize::Large => beta5::PayloadSize::Large,
        });

        group.throughput(Throughput::Bytes(
            buffa::encode_hello_request_json(&buffa_request).len() as u64,
        ));

        group.bench_function(BenchmarkId::new("buffa", size.as_str()), |b| {
            b.iter(|| black_box(buffa::encode_hello_request_json(black_box(&buffa_request))))
        });

        group.bench_function(BenchmarkId::new("beta5", size.as_str()), |b| {
            b.iter(|| black_box(beta5::encode_hello_request_json(black_box(&beta5_request))))
        });
    }

    group.finish();
}

fn json_decode_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_decode_hello_request");

    for size in [
        buffa::PayloadSize::Small,
        buffa::PayloadSize::Medium,
        buffa::PayloadSize::Large,
    ] {
        let buffa_bytes = buffa::encode_hello_request_json(&buffa::hello_request(size));
        let beta5_bytes = beta5::encode_hello_request_json(&beta5::hello_request(match size {
            buffa::PayloadSize::Small => beta5::PayloadSize::Small,
            buffa::PayloadSize::Medium => beta5::PayloadSize::Medium,
            buffa::PayloadSize::Large => beta5::PayloadSize::Large,
        }));

        group.throughput(Throughput::Bytes(buffa_bytes.len() as u64));

        group.bench_function(BenchmarkId::new("buffa", size.as_str()), |b| {
            b.iter(|| black_box(buffa::decode_hello_request_json(black_box(&buffa_bytes))))
        });

        group.bench_function(BenchmarkId::new("beta5", size.as_str()), |b| {
            b.iter(|| black_box(beta5::decode_hello_request_json(black_box(&beta5_bytes))))
        });
    }

    group.finish();
}

fn connect_unary_proto_roundtrip_benchmarks(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let mut group = c.benchmark_group("connect_unary_proto_roundtrip");
    group.sample_size(10);

    for size in [
        buffa::PayloadSize::Small,
        buffa::PayloadSize::Medium,
        buffa::PayloadSize::Large,
    ] {
        let buffa_app = buffa::connect_app();
        let beta5_app = beta5::connect_app();
        let buffa_body = buffa::encode_hello_request_proto(&buffa::hello_request(size));
        let beta5_body = beta5::encode_hello_request_proto(&beta5::hello_request(match size {
            buffa::PayloadSize::Small => beta5::PayloadSize::Small,
            buffa::PayloadSize::Medium => beta5::PayloadSize::Medium,
            buffa::PayloadSize::Large => beta5::PayloadSize::Large,
        }));

        group.throughput(Throughput::Bytes(buffa_body.len() as u64));

        group.bench_function(BenchmarkId::new("buffa", size.as_str()), |b| {
            b.to_async(&rt).iter(|| {
                let app = buffa_app.clone();
                let body = buffa_body.clone();
                async move {
                    let response = app
                        .oneshot(buffa::unary_proto_request(body))
                        .await
                        .expect("service response");
                    assert_eq!(response.status(), StatusCode::OK);
                    let bytes = response
                        .into_body()
                        .collect()
                        .await
                        .expect("collect body")
                        .to_bytes();
                    black_box(buffa::decode_hello_response_proto(&bytes));
                }
            });
        });

        group.bench_function(BenchmarkId::new("beta5", size.as_str()), |b| {
            b.to_async(&rt).iter(|| {
                let app = beta5_app.clone();
                let body = beta5_body.clone();
                async move {
                    let response = app
                        .oneshot(beta5::unary_proto_request(body))
                        .await
                        .expect("service response");
                    assert_eq!(response.status(), StatusCode::OK);
                    let bytes = response
                        .into_body()
                        .collect()
                        .await
                        .expect("collect body")
                        .to_bytes();
                    black_box(beta5::decode_hello_response_proto(&bytes));
                }
            });
        });
    }

    group.finish();
}

fn connect_unary_json_roundtrip_benchmarks(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let mut group = c.benchmark_group("connect_unary_json_roundtrip");
    group.sample_size(10);

    for size in [
        buffa::PayloadSize::Small,
        buffa::PayloadSize::Medium,
        buffa::PayloadSize::Large,
    ] {
        let buffa_app = buffa::connect_app();
        let beta5_app = beta5::connect_app();
        let buffa_body = buffa::encode_hello_request_json(&buffa::hello_request(size));
        let beta5_body = beta5::encode_hello_request_json(&beta5::hello_request(match size {
            buffa::PayloadSize::Small => beta5::PayloadSize::Small,
            buffa::PayloadSize::Medium => beta5::PayloadSize::Medium,
            buffa::PayloadSize::Large => beta5::PayloadSize::Large,
        }));

        group.throughput(Throughput::Bytes(buffa_body.len() as u64));

        group.bench_function(BenchmarkId::new("buffa", size.as_str()), |b| {
            b.to_async(&rt).iter(|| {
                let app = buffa_app.clone();
                let body = buffa_body.clone();
                async move {
                    let response = app
                        .oneshot(buffa::unary_json_request(body))
                        .await
                        .expect("service response");
                    assert_eq!(response.status(), StatusCode::OK);
                    let bytes = response
                        .into_body()
                        .collect()
                        .await
                        .expect("collect body")
                        .to_bytes();
                    black_box(buffa::decode_hello_response_json(&bytes));
                }
            });
        });

        group.bench_function(BenchmarkId::new("beta5", size.as_str()), |b| {
            b.to_async(&rt).iter(|| {
                let app = beta5_app.clone();
                let body = beta5_body.clone();
                async move {
                    let response = app
                        .oneshot(beta5::unary_json_request(body))
                        .await
                        .expect("service response");
                    assert_eq!(response.status(), StatusCode::OK);
                    let bytes = response
                        .into_body()
                        .collect()
                        .await
                        .expect("collect body")
                        .to_bytes();
                    black_box(beta5::decode_hello_response_json(&bytes));
                }
            });
        });
    }

    group.finish();
}

fn connect_stream_proto_roundtrip_benchmarks(c: &mut Criterion) {
    let rt = Runtime::new().expect("create tokio runtime");
    let mut group = c.benchmark_group("connect_stream_proto_roundtrip");
    group.sample_size(10);
    let _stderr_silencer = StderrSilencer::new();

    for size in [
        buffa::PayloadSize::Small,
        buffa::PayloadSize::Medium,
        buffa::PayloadSize::Large,
    ] {
        let buffa_app = buffa::connect_app();
        let beta5_app = beta5::connect_app();
        let buffa_body = buffa::encode_hello_request_proto(&buffa::hello_request(size));
        let beta5_body = beta5::encode_hello_request_proto(&beta5::hello_request(match size {
            buffa::PayloadSize::Small => beta5::PayloadSize::Small,
            buffa::PayloadSize::Medium => beta5::PayloadSize::Medium,
            buffa::PayloadSize::Large => beta5::PayloadSize::Large,
        }));

        group.throughput(Throughput::Bytes(buffa_body.len() as u64));

        group.bench_function(BenchmarkId::new("buffa", size.as_str()), |b| {
            b.to_async(&rt).iter(|| {
                let app = buffa_app.clone();
                let body = buffa_body.clone();
                async move {
                    let response = app
                        .oneshot(buffa::stream_proto_request(body))
                        .await
                        .expect("service response");
                    assert_eq!(response.status(), StatusCode::OK);
                    let bytes = response
                        .into_body()
                        .collect()
                        .await
                        .expect("collect body")
                        .to_bytes();
                    black_box(buffa::parse_streaming_proto_responses(&bytes));
                }
            });
        });

        group.bench_function(BenchmarkId::new("beta5", size.as_str()), |b| {
            b.to_async(&rt).iter(|| {
                let app = beta5_app.clone();
                let body = beta5_body.clone();
                async move {
                    let response = app
                        .oneshot(beta5::stream_proto_request(body))
                        .await
                        .expect("service response");
                    assert_eq!(response.status(), StatusCode::OK);
                    let bytes = response
                        .into_body()
                        .collect()
                        .await
                        .expect("collect body")
                        .to_bytes();
                    black_box(beta5::parse_streaming_proto_responses(&bytes));
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
        proto_encode_benchmarks,
        proto_decode_benchmarks,
        json_encode_benchmarks,
        json_decode_benchmarks,
        connect_unary_proto_roundtrip_benchmarks,
        connect_unary_json_roundtrip_benchmarks,
        connect_stream_proto_roundtrip_benchmarks
}
criterion_main!(benches);

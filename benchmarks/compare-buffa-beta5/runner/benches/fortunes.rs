use anyhow::{Context, Result, bail};
use axum::Router;
use compare_buffa_beta5_cases_buffa as buffa;
use compare_buffa_beta5_cases_connectrpc as connectrust;
use compare_buffa_beta5_cases_connectrpc::fortune::v1::{FortuneServiceClient, GetFortunesRequest};
use compare_buffa_beta5_cases_release as release;
use compare_buffa_beta5_common as common;
use connectrpc::Protocol;
use connectrpc::client::{ClientConfig, HttpClient};
use reqwest::Client as ReqwestClient;
use serde::Deserialize;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::sync::{Mutex, oneshot};

const CONCURRENCY_LEVELS: &[usize] = &[16, 64, 256];
const DEFAULT_WARMUP: Duration = Duration::from_secs(3);
const DEFAULT_MEASUREMENT: Duration = Duration::from_secs(10);
const QUICK_WARMUP: Duration = Duration::from_secs(1);
const QUICK_MEASUREMENT: Duration = Duration::from_secs(3);
const MAX_LATENCY_SAMPLES: usize = 500_000;

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

struct ValkeyContainer {
    name: String,
    addr: String,
}

impl ValkeyContainer {
    fn start() -> Result<Self> {
        let name = format!("compare-buffa-beta5-valkey-{}", std::process::id());
        let output = Command::new("docker")
            .args([
                "run",
                "-d",
                "--rm",
                "-p",
                "127.0.0.1::6379",
                "--name",
                &name,
                "valkey/valkey:8-alpine",
            ])
            .output()
            .context("start valkey container")?;

        if !output.status.success() {
            bail!(
                "docker run failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let output = Command::new("docker")
            .args(["port", &name, "6379"])
            .output()
            .context("inspect valkey port")?;

        if !output.status.success() {
            bail!(
                "docker port failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let addr = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .context("docker port returned no output")?
            .trim()
            .to_string();

        Ok(Self { name, addr })
    }
}

impl Drop for ValkeyContainer {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.name])
            .output();
    }
}

struct BenchSettings {
    warmup: Duration,
    measurement: Duration,
    concurrency_levels: Vec<usize>,
    quick: bool,
}

struct BenchResult {
    benchmark: &'static str,
    implementation: &'static str,
    concurrency: usize,
    rps: f64,
    p50_us: u64,
    p99_us: u64,
}

#[derive(Deserialize)]
struct JsonGetFortunesResponse {
    fortunes: Vec<JsonFortune>,
}

#[derive(Deserialize)]
struct JsonFortune {
    #[serde(default)]
    id: i32,
    message: String,
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

    tokio::time::sleep(Duration::from_millis(25)).await;

    BenchmarkServer {
        base_url: format!("http://{addr}"),
        shutdown: Some(shutdown_tx),
    }
}

fn percentile(latencies: &mut [u64], percentile: f64) -> u64 {
    if latencies.is_empty() {
        return 0;
    }

    latencies.sort_unstable();
    let index = ((latencies.len() as f64 - 1.0) * percentile).round() as usize;
    latencies[index]
}

async fn bench_proto_server(
    implementation: &'static str,
    base_url: &str,
    concurrency: usize,
    settings: &BenchSettings,
) -> BenchResult {
    let config =
        ClientConfig::new(base_url.parse().expect("valid server URL")).protocol(Protocol::Connect);
    let http = HttpClient::plaintext();
    let running = Arc::new(AtomicBool::new(true));
    let count = Arc::new(AtomicU64::new(0));
    let latencies = Arc::new(Mutex::new(Vec::with_capacity(
        MAX_LATENCY_SAMPLES.min(settings.measurement.as_secs() as usize * 20_000),
    )));

    let mut handles = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let client = FortuneServiceClient::new(http.clone(), config.clone());
        let running = Arc::clone(&running);
        let count = Arc::clone(&count);
        let latencies = Arc::clone(&latencies);

        handles.push(tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                let started = Instant::now();
                if client
                    .get_fortunes(GetFortunesRequest::default())
                    .await
                    .is_ok()
                {
                    let elapsed = started.elapsed();
                    let n = count.fetch_add(1, Ordering::Relaxed);
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

    tokio::time::sleep(settings.warmup).await;
    count.store(0, Ordering::Relaxed);
    latencies.lock().await.clear();
    let measure_start = Instant::now();

    tokio::time::sleep(settings.measurement).await;
    running.store(false, Ordering::Relaxed);
    let elapsed = measure_start.elapsed();

    for handle in handles {
        let _ = handle.await;
    }

    let total = count.load(Ordering::Relaxed);
    let rps = total as f64 / elapsed.as_secs_f64();
    let mut samples = latencies.lock().await;

    BenchResult {
        benchmark: "connect-proto",
        implementation,
        concurrency,
        rps,
        p50_us: percentile(&mut samples, 0.50),
        p99_us: percentile(&mut samples, 0.99),
    }
}

async fn bench_json_server(
    implementation: &'static str,
    base_url: &str,
    concurrency: usize,
    settings: &BenchSettings,
) -> BenchResult {
    let client = ReqwestClient::builder()
        .pool_idle_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(64)
        .build()
        .expect("build reqwest client");
    let running = Arc::new(AtomicBool::new(true));
    let count = Arc::new(AtomicU64::new(0));
    let latencies = Arc::new(Mutex::new(Vec::with_capacity(
        MAX_LATENCY_SAMPLES.min(settings.measurement.as_secs() as usize * 20_000),
    )));
    let url = format!("{base_url}{}", buffa::FORTUNE_PATH);

    let mut handles = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let client = client.clone();
        let running = Arc::clone(&running);
        let count = Arc::clone(&count);
        let latencies = Arc::clone(&latencies);
        let url = url.clone();

        handles.push(tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                let started = Instant::now();
                let result = client
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .header("Connect-Protocol-Version", "1")
                    .body("{}")
                    .send()
                    .await;

                match result {
                    Ok(response) if response.status().is_success() => {
                        match response.bytes().await {
                            Ok(bytes) => {
                                match serde_json::from_slice::<JsonGetFortunesResponse>(&bytes) {
                                    Ok(parsed) => {
                                        let sum_ids: i32 =
                                            parsed.fortunes.iter().map(|fortune| fortune.id).sum();
                                        let total_message_bytes: usize = parsed
                                            .fortunes
                                            .iter()
                                            .map(|fortune| fortune.message.len())
                                            .sum();
                                        assert_eq!(
                                            parsed.fortunes.len(),
                                            common::FORTUNES.len() + 1
                                        );
                                        let _ = (sum_ids, total_message_bytes);

                                        let elapsed = started.elapsed();
                                        let n = count.fetch_add(1, Ordering::Relaxed);
                                        if n.is_multiple_of(10) {
                                            let mut samples = latencies.lock().await;
                                            if samples.len() < MAX_LATENCY_SAMPLES {
                                                samples.push(elapsed.as_micros() as u64);
                                            }
                                        }
                                    }
                                    Err(_) => {}
                                }
                            }
                            Err(_) => {}
                        }
                    }
                    Ok(_) => {}
                    Err(_) => {}
                }
            }
        }));
    }

    tokio::time::sleep(settings.warmup).await;
    count.store(0, Ordering::Relaxed);
    latencies.lock().await.clear();
    let measure_start = Instant::now();

    tokio::time::sleep(settings.measurement).await;
    running.store(false, Ordering::Relaxed);
    let elapsed = measure_start.elapsed();

    for handle in handles {
        let _ = handle.await;
    }

    let total = count.load(Ordering::Relaxed);
    let rps = total as f64 / elapsed.as_secs_f64();
    let mut samples = latencies.lock().await;

    BenchResult {
        benchmark: "connect-json",
        implementation,
        concurrency,
        rps,
        p50_us: percentile(&mut samples, 0.50),
        p99_us: percentile(&mut samples, 0.99),
    }
}

async fn prepare_valkey() -> Result<Option<ValkeyContainer>> {
    let Some(valkey_addr) = std::env::var("VALKEY_ADDR").ok() else {
        let container = ValkeyContainer::start()?;
        let deadline = Instant::now() + Duration::from_secs(5);

        loop {
            match common::connect(&container.addr).await {
                Ok(mut conn) => {
                    common::seed(&mut conn).await.context("seed valkey")?;
                    return Ok(Some(container));
                }
                Err(_) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(error) => {
                    bail!("valkey at {} not ready after 5s: {error}", container.addr);
                }
            }
        }
    };

    let mut conn = common::connect(&valkey_addr)
        .await
        .with_context(|| format!("connect to external valkey at {valkey_addr}"))?;
    common::seed(&mut conn)
        .await
        .context("seed external valkey")?;
    Ok(None)
}

fn parse_settings() -> Result<BenchSettings> {
    let mut quick = false;
    let mut concurrency_levels = CONCURRENCY_LEVELS.to_vec();

    for arg in std::env::args().skip(1) {
        if arg == "--bench" {
            continue;
        }

        if arg == "--quick" {
            quick = true;
            continue;
        }

        if let Some(raw) = arg.strip_prefix("--concurrency=") {
            concurrency_levels = raw
                .split(',')
                .filter(|value| !value.is_empty())
                .map(|value| {
                    value
                        .parse::<usize>()
                        .with_context(|| format!("invalid concurrency level {value}"))
                })
                .collect::<Result<Vec<_>>>()?;
            continue;
        }

        bail!("unsupported argument: {arg}");
    }

    if concurrency_levels.is_empty() {
        bail!("at least one concurrency level is required");
    }

    let (warmup, measurement) = if quick {
        (QUICK_WARMUP, QUICK_MEASUREMENT)
    } else {
        (DEFAULT_WARMUP, DEFAULT_MEASUREMENT)
    };

    Ok(BenchSettings {
        warmup,
        measurement,
        concurrency_levels,
        quick,
    })
}

fn print_results(settings: &BenchSettings, results: &[BenchResult]) {
    println!("# Fortune Benchmark");
    println!();
    println!(
        "Benchmarks: connect-proto + connect-json, warmup: {:.1}s, measurement: {:.1}s, mode: {}",
        settings.warmup.as_secs_f64(),
        settings.measurement.as_secs_f64(),
        if settings.quick { "quick" } else { "default" }
    );
    println!();
    println!("| Benchmark | Implementation | Concurrency | req/s | p50 (us) | p99 (us) |");
    println!("|---|---|---:|---:|---:|---:|");

    for result in results {
        println!(
            "| {} | {} | {} | {:.0} | {} | {} |",
            result.benchmark,
            result.implementation,
            result.concurrency,
            result.rps,
            result.p50_us,
            result.p99_us
        );
    }
}

fn main() -> Result<()> {
    let settings = parse_settings()?;
    let runtime = Runtime::new().context("create tokio runtime")?;

    runtime.block_on(async move {
        let valkey_addr = std::env::var("VALKEY_ADDR").ok();
        let container = prepare_valkey().await?;
        let resolved_valkey_addr = valkey_addr
            .unwrap_or_else(|| container.as_ref().expect("container started").addr.clone());

        let buffa_server = spawn_server(buffa::connect_app(&resolved_valkey_addr).await).await;
        let release_server = spawn_server(release::connect_app(&resolved_valkey_addr).await).await;
        let connectrust_server = connectrust::spawn_native_server(&resolved_valkey_addr).await;

        let mut results = Vec::new();
        for &concurrency in &settings.concurrency_levels {
            results.push(
                bench_proto_server("buffa", &buffa_server.base_url, concurrency, &settings).await,
            );
            results.push(
                bench_proto_server("v0.1.0", &release_server.base_url, concurrency, &settings)
                    .await,
            );
            results.push(
                bench_proto_server(
                    "connect-rust",
                    &connectrust_server.base_url,
                    concurrency,
                    &settings,
                )
                .await,
            );
            results.push(
                bench_json_server("buffa", &buffa_server.base_url, concurrency, &settings).await,
            );
            results.push(
                bench_json_server("v0.1.0", &release_server.base_url, concurrency, &settings).await,
            );
            results.push(
                bench_json_server(
                    "connect-rust",
                    &connectrust_server.base_url,
                    concurrency,
                    &settings,
                )
                .await,
            );
        }

        print_results(&settings, &results);
        drop(container);
        Ok(())
    })
}

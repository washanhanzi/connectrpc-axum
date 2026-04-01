use std::io::{BufRead, BufReader, Read};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use connectrpc_axum_benchmarks::support::ValkeyContainer;
use serde::{Deserialize, Serialize};

const CONCURRENCY_LEVELS: &[usize] = &[16, 64, 256];
const HIGH_CONCURRENCY_LEVELS: &[usize] = &[16, 64, 256, 512];
const DEFAULT_WARMUP: Duration = Duration::from_secs(3);
const DEFAULT_MEASUREMENT: Duration = Duration::from_secs(10);
const QUICK_WARMUP: Duration = Duration::from_secs(1);
const QUICK_MEASUREMENT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, Default)]
struct CliOptions {
    quick: bool,
    high_c: bool,
    suites: Vec<Suite>,
    targets: Vec<Target>,
    case_filter: Option<String>,
    json_out: Option<PathBuf>,
}

impl CliOptions {
    fn usage() -> &'static str {
        "usage: benchmarks [--quick] [--high-c] [--suite=<protocol|app>] [--target=<connectrpc_axum|connect_rust|tonic|connect_go>] [--case-filter=<substring>] [--json-out=<path>]"
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Suite {
    ProtocolBenchmarks,
    AppBenchmarks,
}

impl Suite {
    const ALL: [Self; 2] = [Self::ProtocolBenchmarks, Self::AppBenchmarks];

    fn label(self) -> &'static str {
        match self {
            Self::ProtocolBenchmarks => "protocol_benchmarks",
            Self::AppBenchmarks => "app_benchmarks",
        }
    }

    fn short_name(self) -> &'static str {
        match self {
            Self::ProtocolBenchmarks => "protocol",
            Self::AppBenchmarks => "app",
        }
    }

    fn uses_valkey(self) -> bool {
        matches!(self, Self::AppBenchmarks)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Target {
    ConnectrpcAxum,
    ConnectRust,
    Tonic,
    ConnectGo,
}

impl Target {
    const ALL: [Self; 4] = [
        Self::ConnectrpcAxum,
        Self::ConnectRust,
        Self::Tonic,
        Self::ConnectGo,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::ConnectrpcAxum => "connectrpc_axum",
            Self::ConnectRust => "connect_rust",
            Self::Tonic => "tonic",
            Self::ConnectGo => "connect_go",
        }
    }

    fn supports(self, protocol: WireProtocol) -> bool {
        !matches!(self, Self::Tonic) || matches!(protocol, WireProtocol::Grpc)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum WireProtocol {
    ConnectJson,
    ConnectProtobuf,
    Grpc,
}

impl WireProtocol {
    const ALL: [Self; 3] = [Self::ConnectJson, Self::ConnectProtobuf, Self::Grpc];

    fn benchmark_segment(self) -> &'static str {
        match self {
            Self::ConnectJson => "connect_json",
            Self::ConnectProtobuf => "connect_protobuf",
            Self::Grpc => "grpc",
        }
    }

    fn client_flag(self) -> &'static str {
        match self {
            Self::ConnectJson => "connect-json",
            Self::ConnectProtobuf => "connect-protobuf",
            Self::Grpc => "grpc",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PayloadSize {
    Small,
    Medium,
    Large,
}

impl PayloadSize {
    const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];

    fn label(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CompressionMode {
    Identity,
    Gzip,
}

impl CompressionMode {
    const ALL: [Self; 2] = [Self::Identity, Self::Gzip];

    fn label(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Gzip => "gzip",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct BenchmarkCase {
    suite: Suite,
    target: Target,
    protocol: WireProtocol,
    payload_size: PayloadSize,
    compression: CompressionMode,
}

impl BenchmarkCase {
    fn benchmark_name(self) -> String {
        format!(
            "{}_{}_{}_{}",
            self.target.label(),
            self.protocol.benchmark_segment(),
            self.payload_size.label(),
            self.compression.label()
        )
    }
}

struct GoBinaries {
    client: PathBuf,
    protocol_server: PathBuf,
    app_server: PathBuf,
}

struct ServerSpec {
    target: Target,
    binary: PathBuf,
    env: Vec<(String, String)>,
    needs_valkey: bool,
}

struct ServerProcess {
    child: Child,
    addr: SocketAddr,
}

impl ServerProcess {
    fn start(server: &ServerSpec, valkey_addr: Option<&str>) -> Result<Self> {
        let mut command = Command::new(&server.binary);
        if server.needs_valkey {
            command.arg(valkey_addr.context(
                "app benchmark server requested a Valkey address, but none was started",
            )?);
        }
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        for (key, value) in &server.env {
            command.env(key, value);
        }

        let mut child = command
            .spawn()
            .with_context(|| format!("starting {}", server.binary.display()))?;

        let stdout = child
            .stdout
            .take()
            .with_context(|| format!("capturing stdout from {}", server.binary.display()))?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let bytes_read = reader
            .read_line(&mut line)
            .with_context(|| format!("reading server address from {}", server.binary.display()))?;

        if bytes_read == 0 {
            let stderr = read_stderr(&mut child)?;
            bail!(
                "{} exited before printing its address{}",
                server.binary.display(),
                format_stderr(&stderr)
            );
        }

        let addr = line.trim().parse().with_context(|| {
            format!(
                "parsing server address {:?} from {}",
                line.trim(),
                server.binary.display()
            )
        })?;

        std::thread::sleep(Duration::from_millis(50));

        Ok(Self { child, addr })
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Clone, Debug, Serialize)]
struct BenchResult {
    suite: Suite,
    benchmark: String,
    target: Target,
    protocol: WireProtocol,
    payload_size: PayloadSize,
    compression: CompressionMode,
    concurrency: usize,
    rps: f64,
    p50_us: u64,
    p99_us: u64,
}

#[derive(Deserialize)]
struct GoBenchMetrics {
    rps: f64,
    p50_us: u64,
    p99_us: u64,
}

#[derive(Serialize)]
struct BenchRunReport {
    generated_at_unix_ms: u64,
    quick: bool,
    warmup_ms: u64,
    measurement_ms: u64,
    concurrency_levels: Vec<usize>,
    results: Vec<BenchResult>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let options = parse_args()?;
    let levels = if options.high_c {
        HIGH_CONCURRENCY_LEVELS
    } else {
        CONCURRENCY_LEVELS
    };
    let (warmup, measurement) = if options.quick {
        (QUICK_WARMUP, QUICK_MEASUREMENT)
    } else {
        (DEFAULT_WARMUP, DEFAULT_MEASUREMENT)
    };
    let selected_suites = if options.suites.is_empty() {
        Suite::ALL.to_vec()
    } else {
        options.suites.clone()
    };

    eprintln!(
        "Preparing benchmark suites (warmup {:?}, measurement {:?})...",
        warmup, measurement
    );

    let valkey = if selected_suites.iter().any(|suite| suite.uses_valkey()) {
        let container = ValkeyContainer::start().await?;
        eprintln!("Using Valkey at {}", container.addr());
        Some(container)
    } else {
        None
    };

    let go_binaries = GoBinaries {
        client: build_connect_go_binary(
            "./cmd/connect-go-client",
            "connect-go-client",
            "shared connect-go benchmark client",
        )?,
        protocol_server: build_connect_go_binary(
            "./cmd/connect-go-protocol-server",
            "connect-go-protocol-server",
            "connect-go protocol benchmark server",
        )?,
        app_server: build_connect_go_binary(
            "./cmd/connect-go-app-server",
            "connect-go-app-server",
            "connect-go app benchmark server",
        )?,
    };

    let mut all_results = Vec::new();
    for suite in selected_suites {
        let full_case_matrix = benchmark_cases(suite);
        if full_case_matrix.len() != 60 {
            bail!(
                "{} generated {} cases instead of the expected 60",
                suite.label(),
                full_case_matrix.len()
            );
        }

        let filtered_cases: Vec<BenchmarkCase> = full_case_matrix
            .into_iter()
            .filter(|case| options.targets.is_empty() || options.targets.contains(&case.target))
            .filter(|case| {
                options
                    .case_filter
                    .as_ref()
                    .is_none_or(|pattern| case.benchmark_name().contains(pattern))
            })
            .collect();

        if filtered_cases.is_empty() {
            eprintln!(
                "Skipping {} because no benchmark cases matched the current filters.",
                suite.label()
            );
            continue;
        }

        eprintln!(
            "\nRunning {} ({} selected cases)...",
            suite.label(),
            filtered_cases.len()
        );

        let selected_targets_for_suite: Vec<Target> = Target::ALL
            .into_iter()
            .filter(|target| filtered_cases.iter().any(|case| case.target == *target))
            .collect();

        let server_specs = suite_server_specs(suite, &go_binaries, &selected_targets_for_suite)?;
        let mut suite_results = Vec::new();
        for server in server_specs {
            let cases_for_target: Vec<BenchmarkCase> = filtered_cases
                .iter()
                .copied()
                .filter(|case| case.target == server.target)
                .collect();
            if cases_for_target.is_empty() {
                continue;
            }

            let process =
                ServerProcess::start(&server, valkey.as_ref().map(ValkeyContainer::addr))?;
            for case in cases_for_target {
                for &concurrency in levels {
                    let benchmark = case.benchmark_name();
                    eprintln!("  {} @ concurrency={}", benchmark, concurrency);

                    let result = run_connect_go_benchmark(
                        &go_binaries.client,
                        process.addr,
                        case,
                        concurrency,
                        warmup,
                        measurement,
                    )?;

                    eprintln!(
                        "    => {:.0} req/s, p50={:.2}ms, p99={:.2}ms",
                        result.rps,
                        result.p50_us as f64 / 1000.0,
                        result.p99_us as f64 / 1000.0,
                    );
                    suite_results.push(result);
                }
            }
            drop(process);
        }

        print_results(suite, &suite_results);
        all_results.extend(suite_results);
    }

    let report = BenchRunReport {
        generated_at_unix_ms: unix_timestamp_ms()?,
        quick: options.quick,
        warmup_ms: warmup.as_millis() as u64,
        measurement_ms: measurement.as_millis() as u64,
        concurrency_levels: levels.to_vec(),
        results: all_results,
    };
    let json_out = options.json_out.unwrap_or_else(default_json_output_path);
    write_json_report(&json_out, &report)?;
    eprintln!(
        "\nMachine-readable results written to {}",
        json_out.display()
    );

    Ok(())
}

fn parse_args() -> Result<CliOptions> {
    let mut options = CliOptions::default();

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--quick" => options.quick = true,
            "--high-c" => options.high_c = true,
            "--help" | "-h" => {
                println!("{}", CliOptions::usage());
                std::process::exit(0);
            }
            _ => {
                if let Some(value) = arg.strip_prefix("--suite=") {
                    for part in value.split(',').filter(|part| !part.is_empty()) {
                        let suite = parse_suite(part)?;
                        if !options.suites.contains(&suite) {
                            options.suites.push(suite);
                        }
                    }
                } else if let Some(value) = arg.strip_prefix("--target=") {
                    for part in value.split(',').filter(|part| !part.is_empty()) {
                        let target = parse_target(part)?;
                        if !options.targets.contains(&target) {
                            options.targets.push(target);
                        }
                    }
                } else if let Some(value) = arg.strip_prefix("--case-filter=") {
                    options.case_filter = Some(value.to_string());
                } else if let Some(value) = arg.strip_prefix("--json-out=") {
                    options.json_out = Some(PathBuf::from(value));
                } else {
                    bail!("unsupported argument {arg:?}; {}", CliOptions::usage());
                }
            }
        }
    }

    Ok(options)
}

fn parse_suite(value: &str) -> Result<Suite> {
    match value {
        "protocol" | "protocol_benchmarks" => Ok(Suite::ProtocolBenchmarks),
        "app" | "app_benchmarks" => Ok(Suite::AppBenchmarks),
        other => bail!("unsupported suite {other:?}; use protocol or app"),
    }
}

fn parse_target(value: &str) -> Result<Target> {
    match value {
        "connectrpc_axum" => Ok(Target::ConnectrpcAxum),
        "connect_rust" => Ok(Target::ConnectRust),
        "tonic" => Ok(Target::Tonic),
        "connect_go" => Ok(Target::ConnectGo),
        other => bail!(
            "unsupported target {other:?}; use connectrpc_axum, connect_rust, tonic, or connect_go"
        ),
    }
}

fn benchmark_cases(suite: Suite) -> Vec<BenchmarkCase> {
    let mut cases = Vec::new();
    for target in Target::ALL {
        for protocol in WireProtocol::ALL {
            if !target.supports(protocol) {
                continue;
            }
            for payload_size in PayloadSize::ALL {
                for compression in CompressionMode::ALL {
                    cases.push(BenchmarkCase {
                        suite,
                        target,
                        protocol,
                        payload_size,
                        compression,
                    });
                }
            }
        }
    }
    cases
}

fn suite_server_specs(
    suite: Suite,
    go_binaries: &GoBinaries,
    targets: &[Target],
) -> Result<Vec<ServerSpec>> {
    let mut servers = Vec::with_capacity(targets.len());
    for &target in targets {
        let binary = match (suite, target) {
            (Suite::ProtocolBenchmarks, Target::ConnectrpcAxum) => {
                build_workspace_binary("connectrpc_axum_protocol_server")?
            }
            (Suite::AppBenchmarks, Target::ConnectrpcAxum) => {
                build_workspace_binary("connectrpc_axum_app_server")?
            }
            (Suite::ProtocolBenchmarks, Target::ConnectRust) => {
                build_workspace_binary("connect_rust_protocol_server")?
            }
            (Suite::AppBenchmarks, Target::ConnectRust) => {
                build_workspace_binary("connect_rust_app_server")?
            }
            (Suite::ProtocolBenchmarks, Target::Tonic) => {
                build_workspace_binary("tonic_protocol_server")?
            }
            (Suite::AppBenchmarks, Target::Tonic) => build_workspace_binary("tonic_app_server")?,
            (Suite::ProtocolBenchmarks, Target::ConnectGo) => go_binaries.protocol_server.clone(),
            (Suite::AppBenchmarks, Target::ConnectGo) => go_binaries.app_server.clone(),
        };

        servers.push(ServerSpec {
            target,
            binary,
            env: Vec::new(),
            needs_valkey: suite.uses_valkey(),
        });
    }
    Ok(servers)
}

fn print_results(suite: Suite, results: &[BenchResult]) {
    println!();
    println!("{}", suite.label());
    println!(
        "{:<44} {:>12} {:>14} {:>12} {:>12}",
        "Benchmark", "Concurrency", "Requests/sec", "p50 (ms)", "p99 (ms)"
    );
    println!("{}", "-".repeat(98));
    for result in results {
        println!(
            "{:<44} {:>12} {:>14.0} {:>12.2} {:>12.2}",
            result.benchmark,
            result.concurrency,
            result.rps,
            result.p50_us as f64 / 1000.0,
            result.p99_us as f64 / 1000.0,
        );
    }
}

fn run_connect_go_benchmark(
    client_binary: &Path,
    addr: SocketAddr,
    case: BenchmarkCase,
    concurrency: usize,
    warmup: Duration,
    measurement: Duration,
) -> Result<BenchResult> {
    let benchmark = case.benchmark_name();
    let output = Command::new(client_binary)
        .arg("--base-url")
        .arg(format!("http://{addr}"))
        .arg("--suite")
        .arg(case.suite.short_name())
        .arg("--protocol")
        .arg(case.protocol.client_flag())
        .arg("--payload-size")
        .arg(case.payload_size.label())
        .arg("--compression")
        .arg(case.compression.label())
        .arg("--concurrency")
        .arg(concurrency.to_string())
        .arg("--warmup")
        .arg(format_duration_arg(warmup))
        .arg("--measurement")
        .arg(format_duration_arg(measurement))
        .output()
        .with_context(|| format!("running connect-go benchmark client for {benchmark}"))?;

    if !output.status.success() {
        bail!(
            "connect-go benchmark client failed for {benchmark}{}",
            format_output_stderr(&output)
        );
    }

    let metrics: GoBenchMetrics = serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "decoding connect-go benchmark output for {benchmark}; stdout: {}",
            String::from_utf8_lossy(&output.stdout).trim()
        )
    })?;

    Ok(BenchResult {
        suite: case.suite,
        benchmark,
        target: case.target,
        protocol: case.protocol,
        payload_size: case.payload_size,
        compression: case.compression,
        concurrency,
        rps: metrics.rps,
        p50_us: metrics.p50_us,
        p99_us: metrics.p99_us,
    })
}

fn format_duration_arg(duration: Duration) -> String {
    format!("{}ms", duration.as_millis())
}

fn build_workspace_binary(bin_name: &str) -> Result<PathBuf> {
    let workspace_root = workspace_root();
    let status = Command::new("cargo")
        .current_dir(&workspace_root)
        .args([
            "build",
            "--release",
            "-p",
            "connectrpc-axum-benchmarks",
            "--bin",
            bin_name,
            "--message-format=short",
        ])
        .status()
        .with_context(|| format!("building {bin_name}"))?;

    if !status.success() {
        bail!("cargo build failed for {bin_name}");
    }

    Ok(workspace_root.join("target/release").join(bin_name))
}

fn build_connect_go_binary(package: &str, output_name: &str, description: &str) -> Result<PathBuf> {
    let go_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("connect-go");
    let output_dir = benchmarks_target_dir();
    std::fs::create_dir_all(&output_dir).context("creating benchmark target directory")?;
    let binary = output_dir.join(output_name);

    let status = Command::new("go")
        .current_dir(&go_dir)
        .args(["build", "-o"])
        .arg(&binary)
        .arg(package)
        .status()
        .with_context(|| format!("building {description}"))?;

    if !status.success() {
        bail!("go build failed for {description}");
    }

    Ok(binary)
}

fn write_json_report(path: &Path, report: &BenchRunReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating result directory {}", parent.display()))?;
    }

    let bytes = serde_json::to_vec_pretty(report).context("encoding benchmark report as JSON")?;
    std::fs::write(path, bytes)
        .with_context(|| format!("writing benchmark report to {}", path.display()))?;
    Ok(())
}

fn default_json_output_path() -> PathBuf {
    benchmarks_target_dir().join("results/latest.json")
}

fn unix_timestamp_ms() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock was before the Unix epoch")?
        .as_millis() as u64)
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("benchmarks crate should live in the workspace root")
        .to_path_buf()
}

fn benchmarks_target_dir() -> PathBuf {
    workspace_root().join("target/benchmarks")
}

fn read_stderr(child: &mut Child) -> Result<String> {
    let mut stderr = String::new();
    if let Some(mut handle) = child.stderr.take() {
        handle
            .read_to_string(&mut stderr)
            .context("reading server stderr")?;
    }
    Ok(stderr)
}

fn format_output_stderr(output: &Output) -> String {
    format_stderr(&String::from_utf8_lossy(&output.stderr))
}

fn format_stderr(stderr: &str) -> String {
    let stderr = stderr.trim();
    if stderr.is_empty() {
        String::new()
    } else {
        format!("; stderr: {stderr}")
    }
}

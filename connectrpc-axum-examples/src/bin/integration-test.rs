//! Integration Test Runner
//!
//! Orchestrates all tests for connectrpc-axum with dynamic port allocation
//! to prevent port conflicts and ensure test reliability.
//!
//! # Cross-Implementation Testing
//!
//! This test runner supports cross-implementation testing:
//! - **Rust servers** are tested against **Go clients** (existing)
//! - **Rust clients** are tested against **Go server** (cross-impl)
//!
//! # Usage
//!
//! ```bash
//! # Run all tests
//! cargo run --bin integration-test
//!
//! # Run only unit tests
//! cargo run --bin integration-test -- --unit
//!
//! # Run only Rust client tests (against Rust servers)
//! cargo run --bin integration-test -- --rust-client
//!
//! # Run only Go client tests (against Rust servers)
//! cargo run --bin integration-test -- --go-client
//!
//! # Run only cross-implementation tests (Rust clients against Go server)
//! cargo run --bin integration-test -- --cross-impl
//!
//! # Run a specific Go test
//! cargo run --bin integration-test -- --go-client --filter TestConnectUnary
//! ```

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// ANSI color codes for terminal output
mod colors {
    pub const GREEN: &str = "\x1b[32m";
    pub const RED: &str = "\x1b[31m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const CYAN: &str = "\x1b[36m";
    pub const BOLD: &str = "\x1b[1m";
    pub const RESET: &str = "\x1b[0m";
}

/// Test result
#[derive(Debug, Clone)]
struct TestResult {
    name: String,
    passed: bool,
    duration: Duration,
    output: String,
}

/// Configuration for a server binary
#[derive(Debug, Clone)]
struct ServerConfig {
    /// Binary name (e.g., "connect-unary")
    name: &'static str,
    /// Required cargo features (e.g., "tonic")
    features: Option<&'static str>,
}

/// Configuration for a test
#[derive(Debug, Clone)]
struct TestConfig {
    /// Test name for display
    name: &'static str,
    /// Server to start for this test
    server: ServerConfig,
    /// Go test pattern (e.g., "TestConnectUnary")
    go_test_pattern: &'static str,
}

/// Rust client test configuration
#[derive(Debug, Clone)]
struct RustClientTest {
    /// Test name for display
    name: &'static str,
    /// Server binary to start
    server: ServerConfig,
    /// Client binary to run
    client_bin: &'static str,
}

/// Find an available port by binding to port 0
fn find_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind to port 0");
    listener.local_addr().unwrap().port()
}

/// Wait for a server to be ready on the given port
fn wait_for_server(port: u16, timeout: Duration) -> bool {
    let start = Instant::now();
    let addr = format!("127.0.0.1:{}", port);

    while start.elapsed() < timeout {
        if std::net::TcpStream::connect(&addr).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Start a server binary with the given port
fn start_server(
    root_dir: &PathBuf,
    config: &ServerConfig,
    port: u16,
) -> std::io::Result<Child> {
    let mut args = vec![
        "run".to_string(),
        "-p".to_string(),
        "connectrpc-axum-examples".to_string(),
        "--bin".to_string(),
        config.name.to_string(),
    ];

    if let Some(features) = config.features {
        args.push("--features".to_string());
        args.push(features.to_string());
    }

    Command::new("cargo")
        .args(&args)
        .current_dir(root_dir)
        .env("PORT", port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// Kill a server process gracefully
fn stop_server(mut child: Child) {
    // Try SIGTERM first
    let _ = child.kill();

    // Wait up to 2 seconds for graceful shutdown
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // Force kill if still running
    let _ = child.kill();
    let _ = child.wait();
}

/// Start the Go reference server with the given port
fn start_go_server(root_dir: &PathBuf, port: u16) -> std::io::Result<Child> {
    let go_server_dir = root_dir.join("connectrpc-axum-examples/go-server");

    Command::new("go")
        .args(["run", "."])
        .current_dir(&go_server_dir)
        .env("PORT", port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// Run cargo test (unit tests)
fn run_unit_tests(root_dir: &PathBuf) -> TestResult {
    let start = Instant::now();

    let output = Command::new("cargo")
        .args(["test", "--workspace"])
        .current_dir(root_dir)
        .output()
        .expect("Failed to run cargo test");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    TestResult {
        name: "Unit Tests".to_string(),
        passed: output.status.success(),
        duration: start.elapsed(),
        output: combined,
    }
}

/// Run a Rust client test
fn run_rust_client_test(
    root_dir: &PathBuf,
    test: &RustClientTest,
    port: u16,
) -> TestResult {
    let start = Instant::now();
    let server_url = format!("http://localhost:{}", port);

    // Start the server
    let server = match start_server(root_dir, &test.server, port) {
        Ok(s) => s,
        Err(e) => {
            return TestResult {
                name: test.name.to_string(),
                passed: false,
                duration: start.elapsed(),
                output: format!("Failed to start server: {}", e),
            };
        }
    };

    // Wait for server to be ready
    if !wait_for_server(port, Duration::from_secs(30)) {
        stop_server(server);
        return TestResult {
            name: test.name.to_string(),
            passed: false,
            duration: start.elapsed(),
            output: format!("Server {} failed to start on port {}", test.server.name, port),
        };
    }

    // Run the client binary
    let output = Command::new("cargo")
        .args([
            "run",
            "-p",
            "connectrpc-axum-examples",
            "--bin",
            test.client_bin,
        ])
        .current_dir(root_dir)
        .env("SERVER_URL", &server_url)
        .output();

    // Stop the server
    stop_server(server);

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            TestResult {
                name: test.name.to_string(),
                passed: out.status.success(),
                duration: start.elapsed(),
                output: format!("{}\n{}", stdout, stderr),
            }
        }
        Err(e) => TestResult {
            name: test.name.to_string(),
            passed: false,
            duration: start.elapsed(),
            output: format!("Failed to run client: {}", e),
        },
    }
}

/// Run a single Go test
fn run_go_test(
    root_dir: &PathBuf,
    test: &TestConfig,
    port: u16,
) -> TestResult {
    let start = Instant::now();
    let server_url = format!("http://localhost:{}", port);
    let go_client_dir = root_dir.join("connectrpc-axum-examples/go-client");

    // Start the server
    let server = match start_server(root_dir, &test.server, port) {
        Ok(s) => s,
        Err(e) => {
            return TestResult {
                name: test.name.to_string(),
                passed: false,
                duration: start.elapsed(),
                output: format!("Failed to start server: {}", e),
            };
        }
    };

    // Wait for server to be ready
    if !wait_for_server(port, Duration::from_secs(30)) {
        stop_server(server);
        return TestResult {
            name: test.name.to_string(),
            passed: false,
            duration: start.elapsed(),
            output: format!("Server {} failed to start on port {}", test.server.name, port),
        };
    }

    // Run the Go test
    let output = Command::new("go")
        .args([
            "test",
            "-v",
            "-timeout",
            "60s",
            "-run",
            test.go_test_pattern,
        ])
        .current_dir(&go_client_dir)
        .env("SERVER_URL", &server_url)
        .output();

    // Stop the server
    stop_server(server);

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            TestResult {
                name: test.name.to_string(),
                passed: out.status.success(),
                duration: start.elapsed(),
                output: format!("{}\n{}", stdout, stderr),
            }
        }
        Err(e) => TestResult {
            name: test.name.to_string(),
            passed: false,
            duration: start.elapsed(),
            output: format!("Failed to run go test: {}", e),
        },
    }
}

/// Print a test result
fn print_result(result: &TestResult, verbose: bool) {
    let status = if result.passed {
        format!("{}PASS{}", colors::GREEN, colors::RESET)
    } else {
        format!("{}FAIL{}", colors::RED, colors::RESET)
    };

    println!(
        "  {} {} ({:.2}s)",
        status,
        result.name,
        result.duration.as_secs_f64()
    );

    if verbose || !result.passed {
        // Print indented output for failures or in verbose mode
        for line in result.output.lines().take(50) {
            println!("    {}", line);
        }
        if result.output.lines().count() > 50 {
            println!("    ... (output truncated)");
        }
    }
}

/// Get all Go test configurations
fn get_go_tests() -> Vec<TestConfig> {
    vec![
        // Connect-only tests
        TestConfig {
            name: "Go: ConnectUnary",
            server: ServerConfig { name: "connect-unary", features: None },
            go_test_pattern: "TestConnectUnary",
        },
        TestConfig {
            name: "Go: ConnectServerStream",
            server: ServerConfig { name: "connect-server-stream", features: None },
            go_test_pattern: "TestConnectServerStream",
        },
        // Tonic tests (Connect + gRPC)
        TestConfig {
            name: "Go: TonicUnary (Connect + gRPC)",
            server: ServerConfig { name: "tonic-unary", features: Some("tonic") },
            go_test_pattern: "TestTonicUnary",
        },
        TestConfig {
            name: "Go: TonicServerStream",
            server: ServerConfig { name: "tonic-server-stream", features: Some("tonic") },
            go_test_pattern: "TestTonicServerStream",
        },
        TestConfig {
            name: "Go: TonicBidiStream",
            server: ServerConfig { name: "tonic-bidi-stream", features: Some("tonic") },
            go_test_pattern: "TestTonicBidiStream|TestTonicClientStream",
        },
        TestConfig {
            name: "Go: GRPCWeb",
            server: ServerConfig { name: "grpc-web", features: Some("tonic") },
            go_test_pattern: "TestGRPCWeb",
        },
        // Protocol tests
        TestConfig {
            name: "Go: ProtocolVersion",
            server: ServerConfig { name: "protocol-version", features: None },
            go_test_pattern: "TestProtocolVersion",
        },
        TestConfig {
            name: "Go: Timeout",
            server: ServerConfig { name: "timeout", features: None },
            go_test_pattern: "TestTimeout",
        },
        // Extractor tests
        TestConfig {
            name: "Go: ExtractorConnectError",
            server: ServerConfig { name: "extractor-connect-error", features: None },
            go_test_pattern: "TestExtractorConnectError",
        },
        TestConfig {
            name: "Go: ExtractorHTTPResponse",
            server: ServerConfig { name: "extractor-http-response", features: None },
            go_test_pattern: "TestExtractorHTTPResponse",
        },
        TestConfig {
            name: "Go: TonicExtractor",
            server: ServerConfig { name: "tonic-extractor", features: Some("tonic") },
            go_test_pattern: "TestTonicExtractor",
        },
        // Error handling tests
        TestConfig {
            name: "Go: StreamingErrorHandling",
            server: ServerConfig { name: "streaming-error-repro", features: None },
            go_test_pattern: "TestStreamingErrorHandling",
        },
        TestConfig {
            name: "Go: UnaryErrorMetadata",
            server: ServerConfig { name: "unary-error-metadata", features: None },
            go_test_pattern: "TestUnaryErrorMetadata",
        },
        TestConfig {
            name: "Go: ErrorDetails",
            server: ServerConfig { name: "error-details", features: None },
            go_test_pattern: "TestErrorDetails",
        },
        // Size limit tests
        TestConfig {
            name: "Go: ReceiveMaxBytes",
            server: ServerConfig { name: "receive-max-bytes", features: None },
            go_test_pattern: "TestReceiveMaxBytes$",
        },
        TestConfig {
            name: "Go: SendMaxBytes",
            server: ServerConfig { name: "send-max-bytes", features: None },
            go_test_pattern: "TestSendMaxBytes",
        },
        // Compression tests (require compression-gzip-stream feature for gzip support)
        TestConfig {
            name: "Go: StreamingCompression",
            server: ServerConfig { name: "streaming-compression", features: Some("compression-gzip-stream") },
            go_test_pattern: "TestStreamingCompression$",
        },
        TestConfig {
            name: "Go: ClientStreamingCompression",
            server: ServerConfig { name: "client-streaming-compression", features: Some("compression-gzip-stream") },
            go_test_pattern: "TestClientStreamingCompression",
        },
        // Connect bidi/client streaming
        TestConfig {
            name: "Go: ConnectBidiStream",
            server: ServerConfig { name: "connect-bidi-stream", features: None },
            go_test_pattern: "TestConnectBidiStream",
        },
        TestConfig {
            name: "Go: ConnectClientStream",
            server: ServerConfig { name: "connect-client-stream", features: None },
            go_test_pattern: "TestConnectClientStream",
        },
        // Axum router tests
        TestConfig {
            name: "Go: AxumRouter",
            server: ServerConfig { name: "axum-router", features: None },
            go_test_pattern: "TestAxumRouter",
        },
        // GET request tests (require compression-gzip-stream for gzip decompression test)
        TestConfig {
            name: "Go: GetRequest",
            server: ServerConfig { name: "get-request", features: Some("compression-gzip-stream") },
            go_test_pattern: "TestGetRequest",
        },
        // Streaming extractor tests
        TestConfig {
            name: "Go: StreamingExtractor",
            server: ServerConfig { name: "streaming-extractor", features: None },
            go_test_pattern: "TestStreamingExtractor",
        },
        // EndStream metadata tests
        TestConfig {
            name: "Go: EndStreamMetadata",
            server: ServerConfig { name: "endstream-metadata", features: None },
            go_test_pattern: "TestEndStreamMetadata",
        },
    ]
}

/// Get Rust client test configurations (against Rust servers)
fn get_rust_client_tests() -> Vec<RustClientTest> {
    vec![
        RustClientTest {
            name: "Rust Client: Unary",
            server: ServerConfig { name: "connect-unary", features: None },
            client_bin: "unary-client",
        },
        RustClientTest {
            name: "Rust Client: Server Stream",
            server: ServerConfig { name: "connect-server-stream", features: None },
            client_bin: "server-stream-client",
        },
        RustClientTest {
            name: "Rust Client: Client Stream",
            server: ServerConfig { name: "connect-client-stream", features: None },
            client_bin: "client-stream-client",
        },
        RustClientTest {
            name: "Rust Client: Bidi Stream",
            server: ServerConfig { name: "connect-bidi-stream", features: None },
            client_bin: "bidi-stream-client",
        },
    ]
}

/// Cross-implementation test configuration (Rust client against Go server)
#[derive(Debug, Clone)]
struct CrossImplTest {
    /// Test name for display
    name: &'static str,
    /// Rust client binary to run
    client_bin: &'static str,
}

/// Get cross-implementation test configurations (Rust clients against Go server)
fn get_cross_impl_tests() -> Vec<CrossImplTest> {
    vec![
        CrossImplTest {
            name: "Cross-Impl: Rust Unary Client → Go Server",
            client_bin: "unary-client",
        },
        CrossImplTest {
            name: "Cross-Impl: Rust Server Stream Client → Go Server",
            client_bin: "server-stream-client",
        },
        CrossImplTest {
            name: "Cross-Impl: Rust Client Stream Client → Go Server",
            client_bin: "client-stream-client",
        },
        CrossImplTest {
            name: "Cross-Impl: Rust Bidi Stream Client → Go Server",
            client_bin: "bidi-stream-client",
        },
        CrossImplTest {
            name: "Cross-Impl: Rust Typed Client → Go Server",
            client_bin: "typed-client",
        },
    ]
}

/// Run a cross-implementation test (Rust client against Go server)
fn run_cross_impl_test(
    root_dir: &PathBuf,
    test: &CrossImplTest,
    port: u16,
) -> TestResult {
    let start = Instant::now();
    let server_url = format!("http://localhost:{}", port);

    // Start the Go server
    let server = match start_go_server(root_dir, port) {
        Ok(s) => s,
        Err(e) => {
            return TestResult {
                name: test.name.to_string(),
                passed: false,
                duration: start.elapsed(),
                output: format!("Failed to start Go server: {}", e),
            };
        }
    };

    // Wait for server to be ready
    if !wait_for_server(port, Duration::from_secs(30)) {
        stop_server(server);
        return TestResult {
            name: test.name.to_string(),
            passed: false,
            duration: start.elapsed(),
            output: format!("Go server failed to start on port {}", port),
        };
    }

    // Run the Rust client binary
    let output = Command::new("cargo")
        .args([
            "run",
            "-p",
            "connectrpc-axum-examples",
            "--bin",
            test.client_bin,
        ])
        .current_dir(root_dir)
        .env("SERVER_URL", &server_url)
        .output();

    // Stop the server
    stop_server(server);

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            TestResult {
                name: test.name.to_string(),
                passed: out.status.success(),
                duration: start.elapsed(),
                output: format!("{}\n{}", stdout, stderr),
            }
        }
        Err(e) => TestResult {
            name: test.name.to_string(),
            passed: false,
            duration: start.elapsed(),
            output: format!("Failed to run Rust client: {}", e),
        },
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let run_unit = args.contains(&"--unit".to_string()) || args.len() == 1;
    let run_rust_client = args.contains(&"--rust-client".to_string()) || args.len() == 1;
    let run_go_client = args.contains(&"--go-client".to_string()) || args.len() == 1;
    let run_cross_impl = args.contains(&"--cross-impl".to_string()) || args.len() == 1;
    let verbose = args.contains(&"-v".to_string()) || args.contains(&"--verbose".to_string());

    // Get filter if specified
    let filter = args
        .iter()
        .position(|a| a == "--filter")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str());

    // Find project root
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap());

    let root_dir = manifest_dir
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists() && p.join("connectrpc-axum").exists())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| manifest_dir.clone());

    println!(
        "\n{}{}=== ConnectRPC-Axum Integration Tests ==={}\n",
        colors::BOLD, colors::CYAN, colors::RESET
    );
    println!("Project root: {}", root_dir.display());

    let mut results: Vec<TestResult> = Vec::new();
    let mut passed = 0;
    let mut failed = 0;

    // Run unit tests
    if run_unit {
        println!(
            "\n{}--- Unit Tests ---{}",
            colors::YELLOW, colors::RESET
        );
        let result = run_unit_tests(&root_dir);
        print_result(&result, verbose);
        if result.passed {
            passed += 1;
        } else {
            failed += 1;
        }
        results.push(result);
    }

    // Run Rust client tests
    if run_rust_client {
        println!(
            "\n{}--- Rust Client Tests ---{}",
            colors::YELLOW, colors::RESET
        );
        for test in get_rust_client_tests() {
            if let Some(f) = filter {
                if !test.name.contains(f) {
                    continue;
                }
            }

            let port = find_free_port();
            let result = run_rust_client_test(&root_dir, &test, port);
            print_result(&result, verbose);
            if result.passed {
                passed += 1;
            } else {
                failed += 1;
            }
            results.push(result);
        }
    }

    // Run Go client tests
    if run_go_client {
        println!(
            "\n{}--- Go Client Tests (→ Rust Servers) ---{}",
            colors::YELLOW, colors::RESET
        );
        for test in get_go_tests() {
            if let Some(f) = filter {
                if !test.name.contains(f) && !test.go_test_pattern.contains(f) {
                    continue;
                }
            }

            let port = find_free_port();
            let result = run_go_test(&root_dir, &test, port);
            print_result(&result, verbose);
            if result.passed {
                passed += 1;
            } else {
                failed += 1;
            }
            results.push(result);
        }
    }

    // Run cross-implementation tests (Rust clients against Go server)
    if run_cross_impl {
        println!(
            "\n{}--- Cross-Implementation Tests (Rust Clients → Go Server) ---{}",
            colors::YELLOW, colors::RESET
        );
        for test in get_cross_impl_tests() {
            if let Some(f) = filter {
                if !test.name.contains(f) && !test.client_bin.contains(f) {
                    continue;
                }
            }

            let port = find_free_port();
            let result = run_cross_impl_test(&root_dir, &test, port);
            print_result(&result, verbose);
            if result.passed {
                passed += 1;
            } else {
                failed += 1;
            }
            results.push(result);
        }
    }

    // Print summary
    println!(
        "\n{}{}=== Summary ==={}\n",
        colors::BOLD, colors::CYAN, colors::RESET
    );
    println!(
        "{}Passed:{} {}",
        colors::GREEN, colors::RESET, passed
    );
    println!(
        "{}Failed:{} {}",
        colors::RED, colors::RESET, failed
    );
    println!("Total:  {}", passed + failed);

    // Print failed tests
    if failed > 0 {
        println!(
            "\n{}Failed tests:{}",
            colors::RED, colors::RESET
        );
        for result in &results {
            if !result.passed {
                println!("  - {}", result.name);
            }
        }
    }

    // Exit with appropriate code
    std::process::exit(if failed > 0 { 1 } else { 0 });
}

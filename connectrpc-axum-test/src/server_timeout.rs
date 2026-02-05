mod server;
mod client;

use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::socket::TestSocket;
use tokio::process::Command;

pub async fn run(rust_sock: &TestSocket, go_sock: &TestSocket) -> anyhow::Result<()> {
    let go_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("go");

    println!("Building Go binaries...");
    let go_server_bin =
        build_go_binary(&go_dir, "./server_timeout/server/", "timeout-server").await?;
    let go_client_bin =
        build_go_binary(&go_dir, "./server_timeout/client/", "timeout-client").await?;

    // Start both servers
    let rust_listener = rust_sock.bind()?;
    let rust_server = tokio::spawn(server::start(rust_listener));

    let mut go_server = Command::new(&go_server_bin)
        .env("SOCKET_PATH", go_sock.go_addr())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    tokio::try_join!(rust_sock.wait_ready(), go_sock.wait_ready())?;

    println!("=== Timeout Integration Tests ===");

    // Run all 4 client tests concurrently
    let (rs_go, rs_rs, go_go, go_rs) = tokio::join!(
        run_go_client(rust_sock, &go_client_bin),
        client::run_timeout_tests(rust_sock),
        run_go_client(go_sock, &go_client_bin),
        client::run_timeout_tests(go_sock),
    );

    // Stop servers
    rust_server.abort();
    go_server.kill().await.ok();

    // Report results
    let mut total = 0;
    let mut passed = 0;

    let mut report_go = |label: &str, result: anyhow::Result<()>| {
        total += 1;
        match result {
            Ok(()) => {
                println!("  PASS  {label}");
                passed += 1;
            }
            Err(e) => println!("  FAIL  {label}: {e}"),
        }
    };
    report_go("Rust Server + Go Client", rs_go);
    report_go("Go Server + Go Client", go_go);

    let mut report_rust = |label: &str, cases: Vec<client::CaseResult>| {
        for case in &cases {
            total += 1;
            match &case.error {
                None => {
                    println!("  PASS  {label} / {}", case.name);
                    passed += 1;
                }
                Some(e) => println!("  FAIL  {label} / {}: {e}", case.name),
            }
        }
    };
    report_rust("Rust Server + Rust Client", rs_rs);
    report_rust("Go Server + Rust Client", go_rs);

    println!();
    println!("{passed}/{total} passed");

    if passed < total {
        std::process::exit(1);
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

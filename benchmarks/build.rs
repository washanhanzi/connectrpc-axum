use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto");

    connectrpc_axum_build::compile_dir("proto")
        .include_file("protos.rs")
        .with_tonic()
        .compile()?;

    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?).join("connect_rust");
    connectrpc_build::Config::new()
        .files(&["proto/bench.proto", "proto/fortune.proto"])
        .includes(&["proto"])
        .out_dir(out_dir)
        .include_file("_connectrpc.rs")
        .compile()?;

    Ok(())
}

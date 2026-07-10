fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_tonic()
        .with_connect_client()
        .include_file("protos.rs")
        .compile()?;
    Ok(())
}

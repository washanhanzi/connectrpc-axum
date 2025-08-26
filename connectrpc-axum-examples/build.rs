fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Generate ConnectRPC service code only (no gRPC/tonic output)
    // Uses default configuration with serde support for JSON serialization
    connectrpc_axum_build::compile_dir("proto").compile()?;

    Ok(())
}

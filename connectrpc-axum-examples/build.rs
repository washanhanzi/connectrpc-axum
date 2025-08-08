fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("proto")
        .with_tonic()
        .compile()?;
    Ok(())
}

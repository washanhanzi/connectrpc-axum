fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_axum_build::compile_dir("../proto")
        .include_file("protos.rs")
        .compile()?;
    Ok(())
}

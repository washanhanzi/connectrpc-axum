fn main() -> Result<(), Box<dyn std::error::Error>> {
    connectrpc_build::Config::new()
        .files(&["../proto/hello.proto"])
        .includes(&["../proto"])
        .include_file("protos.rs")
        .compile()?;
    Ok(())
}

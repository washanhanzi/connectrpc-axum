pub const CONNECT_RUST_COMMIT: &str = "e3fafcb94fc14daf970224f0eff2ba597c71ae47";

pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/connect_rust/_connectrpc.rs"));
}

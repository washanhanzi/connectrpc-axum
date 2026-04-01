pub mod connect_rust;
pub mod support;

include!(concat!(env!("OUT_DIR"), "/protos.rs"));

pub use bench::v1::*;
pub use fortune::v1::*;

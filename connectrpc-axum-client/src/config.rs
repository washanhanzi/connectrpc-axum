//! Configuration modules for Connect RPC client.
//!
//! This module contains request-level configuration:
//! - [`CallOptions`]: Per-call timeout and headers
//! - [`RetryPolicy`]: Retry behavior with exponential backoff
//! - [`Intercept`]: Request/response interception

mod interceptor;
mod options;
mod retry;

pub use interceptor::{
    Chain, HeaderInterceptor, Intercept, InterceptContext, Interceptor,
};
pub use options::CallOptions;
pub(crate) use options::duration_to_timeout_header;
pub use retry::{defaults, retry, retry_with_policy, ExponentialBackoff, RetryExt, RetryPolicy};

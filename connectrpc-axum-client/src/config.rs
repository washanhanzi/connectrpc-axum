//! Configuration modules for Connect RPC client.
//!
//! This module contains request-level configuration:
//! - [`CallOptions`]: Per-call timeout and headers
//! - [`RetryPolicy`]: Retry behavior with exponential backoff
//! - [`Interceptor`]: Header-level interception (simple, no message bounds)
//! - [`MessageInterceptor`]: Message-level interception with typed access

mod interceptor;
mod options;
mod retry;

pub use interceptor::{
    Chain, ClosureInterceptor, HeaderInterceptor, HeaderWrapper, Interceptor, InterceptorInternal,
    MessageInterceptor, MessageWrapper, RequestContext, ResponseContext, StreamContext, StreamType,
};
pub use options::CallOptions;
pub(crate) use options::duration_to_timeout_header;
pub use retry::{defaults, retry, retry_with_policy, ExponentialBackoff, RetryExt, RetryPolicy};

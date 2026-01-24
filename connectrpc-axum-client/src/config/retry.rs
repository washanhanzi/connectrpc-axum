//! Retry logic with exponential backoff for Connect RPC calls.
//!
//! This module provides configurable retry policies following the
//! [gRPC connection backoff specification](https://github.com/grpc/grpc/blob/master/doc/connection-backoff.md).
//!
//! # Overview
//!
//! The retry system consists of:
//! - [`RetryPolicy`]: Configuration for retry behavior (max attempts, backoff settings)
//! - [`ExponentialBackoff`]: Iterator that yields sleep durations with jitter
//! - [`retry`] and [`retry_with_policy`]: Helper functions for retrying RPC calls
//!
//! # Example
//!
//! ```ignore
//! use connectrpc_axum_client::{ConnectClient, RetryPolicy, retry_with_policy};
//!
//! let client = ConnectClient::builder("http://localhost:3000")
//!     .use_proto()
//!     .build()?;
//!
//! let policy = RetryPolicy::default(); // 3 retries with exponential backoff
//!
//! let response = retry_with_policy(&policy, || async {
//!     client.call_unary::<MyRequest, MyResponse>("service/Method", &request).await
//! }).await?;
//! ```
//!
//! # Retryable Errors
//!
//! Only certain error codes are considered safe to retry:
//! - [`Code::Unavailable`](crate::Code::Unavailable) - Service temporarily unavailable
//! - [`Code::ResourceExhausted`](crate::Code::ResourceExhausted) - Rate limited
//! - [`Code::Aborted`](crate::Code::Aborted) - Transaction aborted, safe to retry
//!
//! Transport errors (connection failures, timeouts) are also retryable.
//!
//! Non-retryable errors (e.g., `InvalidArgument`, `NotFound`, `PermissionDenied`)
//! are returned immediately without retry.

use std::future::Future;
use std::time::Duration;

use crate::ClientError;
use connectrpc_axum_core::Code;

/// Default configuration values based on gRPC connection backoff spec.
/// See: https://github.com/grpc/grpc/blob/master/doc/connection-backoff.md
pub mod defaults {
    use std::time::Duration;

    /// Default initial delay before the first retry.
    pub const BASE_DELAY: Duration = Duration::from_secs(1);

    /// Default multiplier for exponential backoff.
    pub const MULTIPLIER: f64 = 1.6;

    /// Default jitter factor (0.2 means +/- 20%).
    pub const JITTER: f64 = 0.2;

    /// Default maximum delay between retries.
    pub const MAX_DELAY: Duration = Duration::from_secs(120);

    /// Default maximum number of retry attempts.
    pub const MAX_RETRIES: u32 = 3;
}

/// Configuration for retry behavior.
///
/// # Default Values
///
/// The default values follow the gRPC connection backoff specification:
/// - `base_delay`: 1 second
/// - `multiplier`: 1.6
/// - `jitter`: 0.2 (20%)
/// - `max_delay`: 120 seconds
/// - `max_retries`: 3
///
/// # Example
///
/// ```
/// use connectrpc_axum_client::RetryPolicy;
/// use std::time::Duration;
///
/// // Use defaults
/// let policy = RetryPolicy::default();
///
/// // Custom configuration
/// let policy = RetryPolicy::new()
///     .max_retries(5)
///     .base_delay(Duration::from_millis(100))
///     .max_delay(Duration::from_secs(30));
/// ```
#[derive(Clone, Debug)]
pub struct RetryPolicy {
    /// Initial delay before the first retry.
    pub base_delay: Duration,

    /// Multiplier for exponential backoff. Should be >= 1.0.
    pub multiplier: f64,

    /// Jitter factor for randomizing delays. Value between 0.0 and 1.0.
    /// A value of 0.2 means the actual delay will be within +/- 20% of the calculated delay.
    pub jitter: f64,

    /// Maximum delay between retries. The delay will never exceed this value.
    pub max_delay: Duration,

    /// Maximum number of retry attempts (not counting the initial request).
    pub max_retries: u32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            base_delay: defaults::BASE_DELAY,
            multiplier: defaults::MULTIPLIER,
            jitter: defaults::JITTER,
            max_delay: defaults::MAX_DELAY,
            max_retries: defaults::MAX_RETRIES,
        }
    }
}

impl RetryPolicy {
    /// Create a new RetryPolicy with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a retry policy that never retries.
    ///
    /// Useful for disabling retries while keeping the retry infrastructure.
    pub fn no_retry() -> Self {
        Self {
            max_retries: 0,
            ..Default::default()
        }
    }

    /// Create a retry policy for aggressive retrying.
    ///
    /// Uses shorter delays suitable for latency-sensitive operations.
    /// - Base delay: 50ms
    /// - Max delay: 1 second
    /// - Max retries: 5
    pub fn aggressive() -> Self {
        Self {
            base_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(1),
            max_retries: 5,
            ..Default::default()
        }
    }

    /// Create a retry policy for patient retrying.
    ///
    /// Uses longer delays suitable for background operations.
    /// - Base delay: 2 seconds
    /// - Max delay: 5 minutes
    /// - Max retries: 10
    pub fn patient() -> Self {
        Self {
            base_delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(300),
            max_retries: 10,
            ..Default::default()
        }
    }

    /// Set the maximum number of retry attempts.
    pub fn max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Set the initial delay before the first retry.
    pub fn base_delay(mut self, delay: Duration) -> Self {
        self.base_delay = delay;
        self
    }

    /// Set the maximum delay between retries.
    pub fn max_delay(mut self, delay: Duration) -> Self {
        self.max_delay = delay;
        self
    }

    /// Set the backoff multiplier.
    ///
    /// # Panics
    ///
    /// Panics if `multiplier` is less than 1.0.
    pub fn multiplier(mut self, multiplier: f64) -> Self {
        assert!(multiplier >= 1.0, "multiplier must be >= 1.0");
        self.multiplier = multiplier;
        self
    }

    /// Set the jitter factor.
    ///
    /// # Panics
    ///
    /// Panics if `jitter` is not between 0.0 and 1.0.
    pub fn jitter(mut self, jitter: f64) -> Self {
        assert!(
            (0.0..=1.0).contains(&jitter),
            "jitter must be between 0.0 and 1.0"
        );
        self.jitter = jitter;
        self
    }

    /// Validate the policy configuration.
    ///
    /// Returns an error if the configuration is invalid.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.base_delay > self.max_delay {
            return Err("base_delay must not exceed max_delay");
        }
        if self.multiplier < 1.0 {
            return Err("multiplier must be >= 1.0");
        }
        if !(0.0..=1.0).contains(&self.jitter) {
            return Err("jitter must be between 0.0 and 1.0");
        }
        Ok(())
    }

    /// Create an ExponentialBackoff iterator from this policy.
    pub fn backoff(&self) -> ExponentialBackoff {
        ExponentialBackoff::new(self.clone())
    }
}

/// Exponential backoff iterator with jitter.
///
/// Yields increasing sleep durations with randomized jitter.
/// The sequence follows: base * multiplier^attempt with +/- jitter.
///
/// # Example
///
/// ```
/// use connectrpc_axum_client::RetryPolicy;
///
/// let policy = RetryPolicy::new().jitter(0.0); // No jitter for predictable output
/// let mut backoff = policy.backoff();
///
/// // First delay is the base delay
/// let delay1 = backoff.next_delay();
/// // Subsequent delays increase exponentially
/// let delay2 = backoff.next_delay();
/// let delay3 = backoff.next_delay();
/// ```
#[derive(Clone, Debug)]
pub struct ExponentialBackoff {
    policy: RetryPolicy,
    /// Current delay without jitter, stored as f64 to avoid rounding errors.
    current_delay_secs: f64,
    /// Number of attempts made.
    attempts: u32,
}

impl ExponentialBackoff {
    /// Create a new ExponentialBackoff from a RetryPolicy.
    pub fn new(policy: RetryPolicy) -> Self {
        let current_delay_secs = policy.base_delay.as_secs_f64();
        Self {
            policy,
            current_delay_secs,
            attempts: 0,
        }
    }

    /// Reset the backoff to its initial state.
    pub fn reset(&mut self) {
        self.current_delay_secs = self.policy.base_delay.as_secs_f64();
        self.attempts = 0;
    }

    /// Get the number of attempts made so far.
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// Check if more retries are allowed.
    pub fn can_retry(&self) -> bool {
        self.attempts < self.policy.max_retries
    }

    /// Get the next delay duration, applying jitter.
    ///
    /// Returns the delay to wait before the next retry attempt.
    /// Advances the internal state for the next call.
    pub fn next_delay(&mut self) -> Duration {
        let delay = self.current_delay_secs;

        // Apply jitter: delay * (1 + jitter * random(-1, 1))
        let jittered = if self.policy.jitter > 0.0 {
            let jitter_range = self.policy.jitter * 2.0;
            let random_factor = rand::random::<f64>() * jitter_range - self.policy.jitter;
            delay * (1.0 + random_factor)
        } else {
            delay
        };

        // Clamp to max_delay
        let clamped = jittered.min(self.policy.max_delay.as_secs_f64());

        // Update for next iteration
        self.current_delay_secs = (self.current_delay_secs * self.policy.multiplier)
            .min(self.policy.max_delay.as_secs_f64());
        self.attempts += 1;

        Duration::from_secs_f64(clamped.max(0.0))
    }
}

/// Retry a fallible async operation with the default retry policy.
///
/// This is a convenience function that uses [`RetryPolicy::default()`].
/// For custom retry configuration, use [`retry_with_policy`].
///
/// # Type Parameters
///
/// - `F`: A factory function that creates the future for each attempt
/// - `Fut`: The future type returned by the factory
/// - `T`: The success type
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::retry;
///
/// let response = retry(|| async {
///     client.call_unary::<Req, Res>("service/Method", &request).await
/// }).await?;
/// ```
pub async fn retry<F, Fut, T>(f: F) -> Result<T, ClientError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, ClientError>>,
{
    retry_with_policy(&RetryPolicy::default(), f).await
}

/// Retry a fallible async operation with a custom retry policy.
///
/// Only retries on retryable errors (see [`ClientError::is_retryable`]).
/// Non-retryable errors are returned immediately.
///
/// # Arguments
///
/// - `policy`: The retry policy to use
/// - `f`: A factory function that creates the future for each attempt
///
/// # Example
///
/// ```ignore
/// use connectrpc_axum_client::{RetryPolicy, retry_with_policy};
/// use std::time::Duration;
///
/// let policy = RetryPolicy::new()
///     .max_retries(5)
///     .base_delay(Duration::from_millis(100));
///
/// let response = retry_with_policy(&policy, || async {
///     client.call_unary::<Req, Res>("service/Method", &request).await
/// }).await?;
/// ```
pub async fn retry_with_policy<F, Fut, T>(policy: &RetryPolicy, f: F) -> Result<T, ClientError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, ClientError>>,
{
    // Validate policy configuration
    if let Err(msg) = policy.validate() {
        return Err(ClientError::new(Code::InvalidArgument, msg));
    }

    let mut backoff = policy.backoff();

    loop {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) if e.is_retryable() && backoff.can_retry() => {
                let delay = backoff.next_delay();
                #[cfg(feature = "tracing")]
                tracing::debug!(
                    error = %e,
                    attempt = backoff.attempts(),
                    delay_ms = delay.as_millis(),
                    "retrying after transient error"
                );
                tokio::time::sleep(delay).await;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Extension trait for adding retry capabilities to clients.
///
/// This trait is not yet implemented but reserved for future use
/// where the client itself could have built-in retry methods.
#[allow(dead_code)]
pub trait RetryExt {
    /// Retry an operation with the default policy.
    fn with_retry<F, Fut, T>(&self, f: F) -> impl Future<Output = Result<T, ClientError>>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T, ClientError>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_policy_default() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.base_delay, Duration::from_secs(1));
        assert!((policy.multiplier - 1.6).abs() < f64::EPSILON);
        assert!((policy.jitter - 0.2).abs() < f64::EPSILON);
        assert_eq!(policy.max_delay, Duration::from_secs(120));
        assert_eq!(policy.max_retries, 3);
    }

    #[test]
    fn test_retry_policy_no_retry() {
        let policy = RetryPolicy::no_retry();
        assert_eq!(policy.max_retries, 0);
    }

    #[test]
    fn test_retry_policy_aggressive() {
        let policy = RetryPolicy::aggressive();
        assert_eq!(policy.base_delay, Duration::from_millis(50));
        assert_eq!(policy.max_delay, Duration::from_secs(1));
        assert_eq!(policy.max_retries, 5);
    }

    #[test]
    fn test_retry_policy_patient() {
        let policy = RetryPolicy::patient();
        assert_eq!(policy.base_delay, Duration::from_secs(2));
        assert_eq!(policy.max_delay, Duration::from_secs(300));
        assert_eq!(policy.max_retries, 10);
    }

    #[test]
    fn test_retry_policy_builder() {
        let policy = RetryPolicy::new()
            .max_retries(5)
            .base_delay(Duration::from_millis(100))
            .max_delay(Duration::from_secs(10))
            .multiplier(2.0)
            .jitter(0.1);

        assert_eq!(policy.max_retries, 5);
        assert_eq!(policy.base_delay, Duration::from_millis(100));
        assert_eq!(policy.max_delay, Duration::from_secs(10));
        assert!((policy.multiplier - 2.0).abs() < f64::EPSILON);
        assert!((policy.jitter - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn test_retry_policy_validate() {
        let valid = RetryPolicy::default();
        assert!(valid.validate().is_ok());

        let invalid = RetryPolicy {
            base_delay: Duration::from_secs(10),
            max_delay: Duration::from_secs(1),
            ..Default::default()
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    #[should_panic(expected = "multiplier must be >= 1.0")]
    fn test_retry_policy_invalid_multiplier() {
        RetryPolicy::new().multiplier(0.5);
    }

    #[test]
    #[should_panic(expected = "jitter must be between 0.0 and 1.0")]
    fn test_retry_policy_invalid_jitter() {
        RetryPolicy::new().jitter(1.5);
    }

    #[test]
    fn test_exponential_backoff_no_jitter() {
        let policy = RetryPolicy::new()
            .base_delay(Duration::from_secs(1))
            .multiplier(2.0)
            .max_delay(Duration::from_secs(100))
            .jitter(0.0);

        let mut backoff = policy.backoff();

        assert_eq!(backoff.attempts(), 0);
        assert!(backoff.can_retry());

        // First delay should be base_delay
        let delay1 = backoff.next_delay();
        assert_eq!(delay1, Duration::from_secs(1));
        assert_eq!(backoff.attempts(), 1);

        // Second delay: 1 * 2 = 2
        let delay2 = backoff.next_delay();
        assert_eq!(delay2, Duration::from_secs(2));

        // Third delay: 2 * 2 = 4
        let delay3 = backoff.next_delay();
        assert_eq!(delay3, Duration::from_secs(4));
    }

    #[test]
    fn test_exponential_backoff_max_delay_clamping() {
        let policy = RetryPolicy::new()
            .base_delay(Duration::from_secs(10))
            .multiplier(10.0)
            .max_delay(Duration::from_secs(15))
            .jitter(0.0);

        let mut backoff = policy.backoff();

        // First: 10s
        assert_eq!(backoff.next_delay(), Duration::from_secs(10));
        // Second: should be 100s but clamped to 15s
        assert_eq!(backoff.next_delay(), Duration::from_secs(15));
        // Third: still clamped
        assert_eq!(backoff.next_delay(), Duration::from_secs(15));
    }

    #[test]
    fn test_exponential_backoff_with_jitter() {
        let policy = RetryPolicy::new()
            .base_delay(Duration::from_secs(1))
            .multiplier(2.0)
            .max_delay(Duration::from_secs(100))
            .jitter(0.2);

        let mut backoff = policy.backoff();

        // With 20% jitter, delay should be between 0.8s and 1.2s
        let delay = backoff.next_delay();
        assert!(delay >= Duration::from_millis(800));
        assert!(delay <= Duration::from_millis(1200));
    }

    #[test]
    fn test_exponential_backoff_reset() {
        let policy = RetryPolicy::new()
            .base_delay(Duration::from_secs(1))
            .multiplier(2.0)
            .jitter(0.0)
            .max_retries(5);

        let mut backoff = policy.backoff();

        backoff.next_delay();
        backoff.next_delay();
        assert_eq!(backoff.attempts(), 2);

        backoff.reset();
        assert_eq!(backoff.attempts(), 0);
        assert_eq!(backoff.next_delay(), Duration::from_secs(1));
    }

    #[test]
    fn test_exponential_backoff_can_retry() {
        let policy = RetryPolicy::new().max_retries(2).jitter(0.0);
        let mut backoff = policy.backoff();

        assert!(backoff.can_retry()); // 0 attempts
        backoff.next_delay();
        assert!(backoff.can_retry()); // 1 attempt
        backoff.next_delay();
        assert!(!backoff.can_retry()); // 2 attempts (max)
    }

    #[tokio::test]
    async fn test_retry_success_first_attempt() {
        let result = retry(|| async { Ok::<_, ClientError>(42) }).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_retry_non_retryable_error() {
        let result = retry(|| async {
            Err::<i32, _>(ClientError::not_found("resource not found"))
        })
        .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), crate::Code::NotFound);
    }

    #[tokio::test]
    async fn test_retry_with_policy_eventual_success() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = attempts.clone();

        let policy = RetryPolicy::new()
            .max_retries(3)
            .base_delay(Duration::from_millis(1))
            .jitter(0.0);

        let result = retry_with_policy(&policy, || {
            let attempts = attempts_clone.clone();
            async move {
                let current = attempts.fetch_add(1, Ordering::SeqCst);
                if current < 2 {
                    Err(ClientError::unavailable("temporary failure"))
                } else {
                    Ok(42)
                }
            }
        })
        .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_exhausted() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = attempts.clone();

        let policy = RetryPolicy::new()
            .max_retries(2)
            .base_delay(Duration::from_millis(1))
            .jitter(0.0);

        let result = retry_with_policy(&policy, || {
            let attempts = attempts_clone.clone();
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err::<i32, _>(ClientError::unavailable("always failing"))
            }
        })
        .await;

        assert!(result.is_err());
        // Initial attempt + 2 retries = 3 total
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }
}

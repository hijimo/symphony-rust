//! Exponential backoff retry logic for platform API calls.
//!
//! Depends on: `crate::error::PlatformError` (implemented by another agent in src/error.rs)

use std::future::Future;
use std::time::Duration;
use tokio::time::sleep;

use crate::error::PlatformError;

/// Maximum number of retry attempts before giving up.
const MAX_RETRIES: u32 = 3;

/// Base delay for exponential backoff (doubles each attempt).
const BASE_DELAY_MS: u64 = 1_000;

/// Executes an async operation with exponential backoff retry.
///
/// Only retries on errors where `PlatformError::is_retryable()` returns true.
/// For rate-limited responses, uses the server-provided `retry_after_ms` as the delay.
/// For other retryable errors, uses exponential backoff: 1s, 2s, 4s.
///
/// # Type bounds
///
/// - `F: Fn() -> Fut + Send + Sync` — the closure must be callable multiple times and safe to
///   send across threads.
/// - `Fut: Future<Output = Result<T, PlatformError>> + Send` — the future must be sendable
///   (required for use inside `tokio::spawn`).
/// - `T: Send` — the success value must be sendable.
pub async fn with_retry<F, Fut, T>(f: F) -> Result<T, PlatformError>
where
    F: Fn() -> Fut + Send + Sync,
    Fut: Future<Output = Result<T, PlatformError>> + Send,
    T: Send,
{
    let mut attempt = 0u32;
    loop {
        match f().await {
            Ok(val) => return Ok(val),
            Err(e) if e.is_retryable() && attempt < MAX_RETRIES => {
                let delay = match &e {
                    PlatformError::RateLimited { retry_after_ms } => {
                        Duration::from_millis(*retry_after_ms)
                    }
                    _ => Duration::from_millis(BASE_DELAY_MS * 2u64.pow(attempt)),
                };
                tracing::debug!(
                    attempt,
                    delay_ms = delay.as_millis() as u64,
                    error = %e,
                    "Retrying after transient error"
                );
                sleep(delay).await;
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_succeeds_immediately() {
        let result = with_retry(|| async { Ok::<_, PlatformError>(42) }).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_retries_on_server_error() {
        let call_count = Arc::new(AtomicU32::new(0));
        let count = Arc::clone(&call_count);

        let result = with_retry(move || {
            let count = Arc::clone(&count);
            async move {
                let n = count.fetch_add(1, Ordering::SeqCst);
                if n < 2 {
                    Err(PlatformError::ServerError(500))
                } else {
                    Ok(99)
                }
            }
        })
        .await;

        assert_eq!(result.unwrap(), 99);
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_does_not_retry_non_retryable() {
        let call_count = Arc::new(AtomicU32::new(0));
        let count = Arc::clone(&call_count);

        let result: Result<i32, _> = with_retry(move || {
            let count = Arc::clone(&count);
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Err(PlatformError::NotFound("issue #1".into()))
            }
        })
        .await;

        assert!(result.is_err());
        // Should only be called once — no retry for NotFound
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_exhausts_retries() {
        let call_count = Arc::new(AtomicU32::new(0));
        let count = Arc::clone(&call_count);

        let result: Result<i32, _> = with_retry(move || {
            let count = Arc::clone(&count);
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Err(PlatformError::Timeout)
            }
        })
        .await;

        assert!(result.is_err());
        // 1 initial + 3 retries = 4 total calls
        assert_eq!(call_count.load(Ordering::SeqCst), MAX_RETRIES + 1);
    }
}

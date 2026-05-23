//! Retry logic unit tests.
//!
//! Tests the exponential backoff retry mechanism, verifying that:
//! - Retryable errors (5xx, timeout) trigger retries
//! - Non-retryable errors (401, 404) fail immediately
//! - Max retry limit is respected
//! - Successful retry on Nth attempt works correctly

#![allow(dead_code)]

mod common;

use serde_json::json;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// =============================================================================
// Retry implementation (mirrors production `with_retry`)
// =============================================================================

#[derive(Debug, Clone)]
enum RetryError {
    Retryable(u16),
    NonRetryable(u16),
    Network(String),
}

impl RetryError {
    fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable(_))
    }

    fn from_status(status: u16) -> Self {
        match status {
            401 | 403 | 404 | 422 => Self::NonRetryable(status),
            429 | 500 | 502 | 503 | 504 => Self::Retryable(status),
            _ => Self::NonRetryable(status),
        }
    }
}

const MAX_RETRIES: u32 = 3;
const BASE_DELAY_MS: u64 = 50; // Reduced for tests (production uses 1000ms)

async fn with_retry<F, Fut, T>(f: F) -> Result<T, RetryError>
where
    F: Fn() -> Fut + Send,
    Fut: std::future::Future<Output = Result<T, RetryError>> + Send,
    T: Send,
{
    let mut attempt = 0;
    loop {
        match f().await {
            Ok(val) => return Ok(val),
            Err(e) if e.is_retryable() && attempt < MAX_RETRIES => {
                let delay = Duration::from_millis(BASE_DELAY_MS * 2u64.pow(attempt));
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

// =============================================================================
// Helper: Simple HTTP fetcher for retry testing
// =============================================================================

async fn fetch_with_status(base_url: &str, path_str: &str) -> Result<String, RetryError> {
    let client = reqwest::Client::new();
    let url = format!("{}{}", base_url, path_str);

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| RetryError::Network(e.to_string()))?;

    let status = resp.status().as_u16();
    if resp.status().is_success() {
        let body = resp
            .text()
            .await
            .map_err(|e| RetryError::Network(e.to_string()))?;
        Ok(body)
    } else {
        Err(RetryError::from_status(status))
    }
}

// =============================================================================
// Tests
// =============================================================================

#[tokio::test]
async fn test_retry_succeeds_on_third_attempt() {
    let mock_server = MockServer::start().await;

    // First 2 requests return 500 (retryable), then succeed.
    // wiremock uses priority: lower number = higher priority, consumed first with up_to_n_times.
    Mock::given(method("GET"))
        .and(path("/api/test"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .up_to_n_times(2)
        .expect(2)
        .with_priority(1) // Higher priority, consumed first
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})))
        .expect(1)
        .with_priority(2) // Lower priority, used after 500s are consumed
        .mount(&mock_server)
        .await;

    let base_url = mock_server.uri();
    let result = with_retry(|| fetch_with_status(&base_url, "/api/test")).await;

    assert!(result.is_ok());
    let body = result.unwrap();
    assert!(body.contains("ok"));
}

#[tokio::test]
async fn test_no_retry_on_401() {
    let mock_server = MockServer::start().await;

    // 401 is non-retryable — should fail immediately without retrying
    Mock::given(method("GET"))
        .and(path("/api/auth"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "message": "Bad credentials"
        })))
        .expect(1) // Should only be called once (no retry)
        .mount(&mock_server)
        .await;

    let base_url = mock_server.uri();
    let result = with_retry(|| fetch_with_status(&base_url, "/api/auth")).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        RetryError::NonRetryable(status) => assert_eq!(status, 401),
        other => panic!("Expected NonRetryable(401), got {:?}", other),
    }
}

#[tokio::test]
async fn test_no_retry_on_404() {
    let mock_server = MockServer::start().await;

    // 404 is non-retryable — resource doesn't exist, retrying won't help
    Mock::given(method("GET"))
        .and(path("/api/missing"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "message": "Not Found"
        })))
        .expect(1) // Should only be called once (no retry)
        .mount(&mock_server)
        .await;

    let base_url = mock_server.uri();
    let result = with_retry(|| fetch_with_status(&base_url, "/api/missing")).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        RetryError::NonRetryable(status) => assert_eq!(status, 404),
        other => panic!("Expected NonRetryable(404), got {:?}", other),
    }
}

#[tokio::test]
async fn test_retry_respects_max_retries() {
    let mock_server = MockServer::start().await;

    // Always return 500 — should retry MAX_RETRIES times then give up.
    // Total calls = 1 (initial) + MAX_RETRIES (retries) = 4
    Mock::given(method("GET"))
        .and(path("/api/always-fail"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Server Error"))
        .expect(4) // 1 initial + 3 retries
        .mount(&mock_server)
        .await;

    let base_url = mock_server.uri();
    let result = with_retry(|| fetch_with_status(&base_url, "/api/always-fail")).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        RetryError::Retryable(status) => assert_eq!(status, 500),
        other => panic!("Expected Retryable(500) after max retries, got {:?}", other),
    }
}

#[tokio::test]
async fn test_retry_on_503_service_unavailable() {
    let mock_server = MockServer::start().await;

    // 503 is retryable — service temporarily unavailable
    Mock::given(method("GET"))
        .and(path("/api/service"))
        .respond_with(ResponseTemplate::new(503).set_body_string("Service Unavailable"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/service"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"recovered": true})))
        .with_priority(2)
        .mount(&mock_server)
        .await;

    let base_url = mock_server.uri();
    let result = with_retry(|| fetch_with_status(&base_url, "/api/service")).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_retry_on_429_rate_limited() {
    let mock_server = MockServer::start().await;

    // 429 is retryable — rate limited
    Mock::given(method("GET"))
        .and(path("/api/rate-limited"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_string("Rate Limited")
                .append_header("retry-after", "1"),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/rate-limited"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": "success"})))
        .with_priority(2)
        .mount(&mock_server)
        .await;

    let base_url = mock_server.uri();
    let result = with_retry(|| fetch_with_status(&base_url, "/api/rate-limited")).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_immediate_success_no_retry() {
    let mock_server = MockServer::start().await;

    // Immediate success — no retries needed
    Mock::given(method("GET"))
        .and(path("/api/healthy"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"healthy": true})))
        .expect(1) // Called exactly once
        .mount(&mock_server)
        .await;

    let base_url = mock_server.uri();
    let result = with_retry(|| fetch_with_status(&base_url, "/api/healthy")).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_no_retry_on_422_unprocessable() {
    let mock_server = MockServer::start().await;

    // 422 is non-retryable — client error, request is malformed
    Mock::given(method("GET"))
        .and(path("/api/invalid"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "message": "Validation Failed",
            "errors": [{"field": "title", "code": "missing"}]
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let base_url = mock_server.uri();
    let result = with_retry(|| fetch_with_status(&base_url, "/api/invalid")).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        RetryError::NonRetryable(status) => assert_eq!(status, 422),
        other => panic!("Expected NonRetryable(422), got {:?}", other),
    }
}

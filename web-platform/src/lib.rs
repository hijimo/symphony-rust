#![allow(
    clippy::manual_ok_err,
    clippy::manual_range_contains,
    clippy::needless_borrows_for_generic_args,
    clippy::new_without_default,
    clippy::should_implement_trait,
    clippy::too_many_arguments,
    clippy::unnecessary_map_or
)]

pub mod alert;
pub mod auth;
pub mod concurrency;
pub mod config;
pub mod crypto;
pub mod db;
pub mod error;
pub mod git_url;
pub mod handlers;
pub mod middleware;
pub mod models;
pub mod notification;
pub mod process_manager;
pub mod repository;
pub mod router;
pub mod services;
pub mod shutdown;
pub mod templates;

use chrono::Utc;
use dashmap::DashMap;
use std::sync::Arc;

use crate::auth::rate_limit::RateLimiter;
use crate::concurrency::ConcurrencyManager;
use crate::notification::NotificationDispatcher;
use crate::process_manager::ProcessManager;
use crate::repository::SqliteRepository;
use crate::services::ai_service::AiService;
use crate::services::cache::ApiCache;

/// Manages the alert engine lifecycle and provides access to notification dispatch.
#[derive(Clone)]
pub struct AlertManager {
    /// Notification dispatcher for sending alerts and test notifications.
    pub dispatcher: Arc<NotificationDispatcher>,
    /// Shutdown signal sender for the alert engine background task.
    pub shutdown_tx: tokio::sync::mpsc::Sender<()>,
}

impl AlertManager {
    pub fn new(
        dispatcher: Arc<NotificationDispatcher>,
        shutdown_tx: tokio::sync::mpsc::Sender<()>,
    ) -> Self {
        Self {
            dispatcher,
            shutdown_tx,
        }
    }

    /// Reload alert rules (called after admin updates rules via API).
    pub async fn reload_rules(&self) -> Result<(), crate::error::WebPlatformError> {
        // Rules are loaded from DB on each evaluation cycle, so this is a no-op
        // for now. In a future iteration, we could signal the engine to reload immediately.
        tracing::info!("Alert rules reload requested");
        Ok(())
    }

    /// Reload notification channels from DB and rebuild the dispatcher's channel list.
    pub async fn reload_channels(
        &self,
        repo: &SqliteRepository,
        encryption_key: &[u8; 32],
    ) -> Result<(), crate::error::WebPlatformError> {
        use crate::notification::DingTalkChannel;
        use crate::repository::AlertRepository;

        let rows = repo.get_all_notification_channels().await?;

        self.dispatcher.clear_channels().await;

        for row in rows {
            if row.channel_type != "dingtalk" {
                continue;
            }

            let config_json = decrypt_channel_config(&row.config_encrypted, encryption_key)?;
            let config: std::collections::HashMap<String, serde_json::Value> =
                serde_json::from_str(&config_json).unwrap_or_default();

            let webhook_url = config
                .get("webhook_url")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let secret = config
                .get("secret")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let severity_filter: Vec<String> =
                serde_json::from_str(&row.severity_filter_json).unwrap_or_default();

            let channel = Arc::new(DingTalkChannel::new(row.channel_id, webhook_url, secret));

            self.dispatcher
                .add_channel(channel, severity_filter, row.enabled)
                .await;
        }

        tracing::info!("Notification channels reloaded");
        Ok(())
    }
}

/// Decrypt channel config (helper used by AlertManager).
fn decrypt_channel_config(
    encrypted: &str,
    key: &[u8; 32],
) -> Result<String, crate::error::WebPlatformError> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    use base64::Engine;

    let combined = base64::engine::general_purpose::STANDARD
        .decode(encrypted)
        .map_err(|e| {
            crate::error::WebPlatformError::Internal(format!("Failed to decode config: {}", e))
        })?;

    if combined.len() < 12 {
        return Err(crate::error::WebPlatformError::Internal(
            "Invalid encrypted config: too short".to_string(),
        ));
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(key).expect("valid key length");
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher.decrypt(nonce, ciphertext).map_err(|e| {
        crate::error::WebPlatformError::Internal(format!("Failed to decrypt config: {}", e))
    })?;

    String::from_utf8(plaintext).map_err(|e| {
        crate::error::WebPlatformError::Internal(format!("Invalid UTF-8 in config: {}", e))
    })
}

#[derive(Clone)]
pub struct AppState {
    pub repo: SqliteRepository,
    pub jwt_secret: String,
    pub encryption_key: [u8; 32],
    pub token_blacklist: Arc<DashMap<i64, chrono::DateTime<Utc>>>,
    pub rate_limiter: Arc<RateLimiter>,
    pub process_manager: ProcessManager,
    /// Singleflight cache for external API calls (Phase 3).
    pub api_cache: Arc<ApiCache>,
    /// AI service for issue generation (Phase 3). None if not configured.
    pub ai_service: Option<Arc<AiService>>,
    /// Rate limiter for Phase 3 endpoints (per-user sliding window).
    pub phase3_rate_limiter: Arc<Phase3RateLimiter>,
    /// Concurrency manager for Phase 4 global/per-project agent tracking.
    pub concurrency_manager: Arc<ConcurrencyManager>,
    /// Path to the symphony-platform binary.
    pub symphony_bin: String,
    /// Root directory for project workspaces.
    pub workspace_root: String,
    /// Alert manager for Phase 5 notification dispatch. None until initialized.
    pub alert_manager: Option<AlertManager>,
}

/// Per-endpoint rate limiter for Phase 3 using sliding window counters.
pub struct Phase3RateLimiter {
    /// Key format: "{endpoint}:{user_id}" -> VecDeque<Instant>
    entries: DashMap<String, std::collections::VecDeque<std::time::Instant>>,
    /// Key: "ai_global" -> VecDeque<Instant> for global AI rate limit
    global_entries: DashMap<String, std::collections::VecDeque<std::time::Instant>>,
}

impl Default for Phase3RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl Phase3RateLimiter {
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
            global_entries: DashMap::new(),
        }
    }

    /// Check rate limit for a specific user and endpoint.
    /// Returns Ok(()) if allowed, Err(seconds_until_retry) if rate limited.
    pub fn check(&self, endpoint: &str, user_id: i64, max_per_minute: u32) -> Result<(), u64> {
        let key = format!("{}:{}", endpoint, user_id);
        let now = std::time::Instant::now();
        let window = std::time::Duration::from_secs(60);

        let mut entry = self.entries.entry(key).or_default();
        // Remove entries outside the window
        while entry
            .front()
            .is_some_and(|t| now.duration_since(*t) > window)
        {
            entry.pop_front();
        }

        if entry.len() >= max_per_minute as usize {
            // Calculate retry-after
            let oldest = entry.front().unwrap();
            let retry_after = window.saturating_sub(now.duration_since(*oldest));
            return Err(retry_after.as_secs().max(1));
        }

        entry.push_back(now);
        Ok(())
    }

    /// Check global rate limit (shared across all users).
    pub fn check_global(&self, key: &str, max_per_minute: u32) -> Result<(), u64> {
        let now = std::time::Instant::now();
        let window = std::time::Duration::from_secs(60);

        let mut entry = self.global_entries.entry(key.to_string()).or_default();

        while entry
            .front()
            .is_some_and(|t| now.duration_since(*t) > window)
        {
            entry.pop_front();
        }

        if entry.len() >= max_per_minute as usize {
            let oldest = entry.front().unwrap();
            let retry_after = window.saturating_sub(now.duration_since(*oldest));
            return Err(retry_after.as_secs().max(1));
        }

        entry.push_back(now);
        Ok(())
    }

    /// Check if a user currently has an in-flight AI generation.
    /// Uses a simple presence check in a dedicated map.
    pub fn has_active_generation(&self, user_id: i64) -> bool {
        let key = format!("ai_active:{}", user_id);
        self.entries.contains_key(&key)
    }

    /// Mark that a user has started an AI generation.
    pub fn start_generation(&self, user_id: i64) {
        let key = format!("ai_active:{}", user_id);
        let mut q = std::collections::VecDeque::new();
        q.push_back(std::time::Instant::now());
        self.entries.insert(key, q);
    }

    /// Mark that a user's AI generation has completed.
    pub fn end_generation(&self, user_id: i64) {
        let key = format!("ai_active:{}", user_id);
        self.entries.remove(&key);
    }
}

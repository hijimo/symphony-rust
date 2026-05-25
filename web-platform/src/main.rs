use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use dashmap::DashMap;
use http::header::{AUTHORIZATION, CONTENT_TYPE};
use http::Method;
use tokio::net::TcpListener;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::EnvFilter;

use web_platform::alert::AlertEngineBuilder;
use web_platform::auth::rate_limit::RateLimiter;
use web_platform::concurrency::ConcurrencyManager;
use web_platform::config::AppConfig;
use web_platform::crypto;
use web_platform::db;
use web_platform::notification::NotificationDispatcher;
use web_platform::process_manager::ProcessManager;
use web_platform::repository::{
    AlertRepository, SqliteRepository, TokenBlacklistRepository, UserRepository,
};
use web_platform::router::create_router;
use web_platform::services::ai_service::{AiService, AiServiceConfig};
use web_platform::services::cache::ApiCache;
use web_platform::shutdown::shutdown_signal;
use web_platform::{AlertManager, AppState, Phase3RateLimiter};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("web_platform=info".parse().unwrap()),
        )
        .init();

    let config = AppConfig::from_env();
    let pool = db::init_pool(&config.database_url);
    let repo = SqliteRepository::new(pool);

    seed_admin(&repo).await;

    let encryption_key =
        crypto::parse_base64_key(&config.encryption_key).expect("Failed to parse ENCRYPTION_KEY");

    let token_blacklist = Arc::new(DashMap::new());

    if let Ok(entries) = repo.load_all().await {
        for entry in entries {
            let ts = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
                entry.invalidated_at,
                chrono::Utc,
            );
            token_blacklist.insert(entry.user_id, ts);
        }
        info!("Loaded token blacklist into memory");
    }

    // Initialize process manager and run startup cleanup
    let process_manager = ProcessManager::new();
    web_platform::process_manager::cleanup::startup_cleanup(&repo).await;

    // Initialize Phase 3 services
    let kanban_cache_ttl: u64 = env::var("KANBAN_CACHE_TTL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let api_cache = Arc::new(ApiCache::new(kanban_cache_ttl, 3, 10000));

    let ai_service = AiServiceConfig::from_env().map(|config| {
        info!("AI service configured (model: {})", config.model);
        Arc::new(AiService::new(config))
    });
    if ai_service.is_none() {
        info!("AI service not configured (AZURE_OPENAI_BASEURL/API_KEY not set)");
    }

    let phase3_rate_limiter = Arc::new(Phase3RateLimiter::new());

    // Initialize Phase 4 concurrency manager
    let global_max: i64 = env::var("MAX_CONCURRENT_CODEX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let concurrency_manager = Arc::new(ConcurrencyManager::new(global_max));
    info!(
        "Concurrency manager initialized (global_max: {})",
        global_max
    );

    // Initialize Phase 5 alert engine via builder
    let dispatcher = Arc::new(NotificationDispatcher::new());

    // Load notification channels from DB and populate the dispatcher
    {
        use web_platform::notification::DingTalkChannel;
        match repo.get_all_notification_channels().await {
            Ok(rows) => {
                let proxy_config =
                    match web_platform::handlers::network_proxy::load_effective_proxy_config(
                        &repo,
                        &encryption_key,
                    )
                    .await
                    {
                        Ok(config) => Some(config),
                        Err(e) => {
                            tracing::warn!("Failed to load proxy config for notifications: {}", e);
                            None
                        }
                    };
                for row in rows {
                    if row.channel_type != "dingtalk" {
                        continue;
                    }
                    let config_json = match decrypt_channel_config_startup(
                        &row.config_encrypted,
                        &encryption_key,
                    ) {
                        Ok(json) => json,
                        Err(e) => {
                            tracing::warn!("Skipping channel {}: {}", row.channel_id, e);
                            continue;
                        }
                    };
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

                    let channel = Arc::new(DingTalkChannel::new_with_proxy(
                        row.channel_id,
                        webhook_url,
                        secret,
                        proxy_config.as_ref(),
                    ));

                    dispatcher
                        .add_channel(channel, severity_filter, row.enabled)
                        .await;
                }
                info!("Notification channels loaded");
            }
            Err(e) => {
                tracing::warn!("Failed to load notification channels: {}", e);
            }
        }
    }

    // Build and spawn the AlertEngine background task
    let alert_eval_interval: u64 = env::var("ALERT_EVAL_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);

    let alert_handle = AlertEngineBuilder::new(
        process_manager.clone(),
        concurrency_manager.clone(),
        repo.clone(),
    )
    .evaluation_interval_secs(alert_eval_interval)
    .build()
    .await;

    let alert_manager = AlertManager::new(dispatcher.clone(), alert_handle.shutdown_tx);
    info!(
        "Alert engine background task spawned (interval: {}s)",
        alert_eval_interval
    );

    let state = AppState {
        repo,
        jwt_secret: config.jwt_secret,
        encryption_key,
        token_blacklist,
        rate_limiter: Arc::new(RateLimiter::new()),
        process_manager,
        api_cache,
        ai_service,
        phase3_rate_limiter,
        concurrency_manager,
        symphony_bin: config.symphony_bin,
        workspace_root: config.workspace_root,
        alert_manager: Some(alert_manager),
    };

    web_platform::services::mr_create::spawn_merge_request_reconciler(state.clone());

    let cors_origin =
        env::var("CORS_ORIGIN").unwrap_or_else(|_| "http://localhost:5177".to_string());
    let cors = CorsLayer::new()
        .allow_origin(
            cors_origin
                .parse::<http::HeaderValue>()
                .map(AllowOrigin::exact)
                .unwrap_or_else(|_| AllowOrigin::exact("http://localhost:5177".parse().unwrap())),
        )
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE]);

    let app = create_router(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .into_make_service_with_connect_info::<SocketAddr>();

    let addr = format!("{}:{}", config.server_host, config.server_port);
    let listener = TcpListener::bind(&addr)
        .await
        .expect("Failed to bind address");
    info!("Server listening on {}", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Server error");

    info!("Server shut down gracefully");
}

async fn seed_admin(repo: &SqliteRepository) {
    use rand::Rng;
    use web_platform::auth::password::hash_password;

    let existing = repo.find_by_username("admin").await;
    if let Ok(None) = existing {
        let password = env::var("ADMIN_INIT_PASSWORD").unwrap_or_else(|_| {
            let mut rng = rand::thread_rng();
            let chars: Vec<char> = (0..24)
                .map(|_| {
                    let idx = rng.gen_range(0..62);
                    match idx {
                        0..=9 => (b'0' + idx) as char,
                        10..=35 => (b'a' + idx - 10) as char,
                        _ => (b'A' + idx - 36) as char,
                    }
                })
                .collect();
            chars.into_iter().collect()
        });

        let password_hash = hash_password(&password).expect("Failed to hash admin password");

        repo.create_user("admin", &password_hash, Some("Administrator"), "admin")
            .await
            .expect("Failed to create admin user");

        info!("Default admin user created. Initial password: {}", password);
    }
}

/// Decrypt channel config at startup (standalone helper to avoid importing lib internals).
fn decrypt_channel_config_startup(encrypted: &str, key: &[u8; 32]) -> Result<String, String> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    use base64::Engine;

    let combined = base64::engine::general_purpose::STANDARD
        .decode(encrypted)
        .map_err(|e| format!("base64 decode failed: {}", e))?;

    if combined.len() < 12 {
        return Err("encrypted data too short".to_string());
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("invalid key: {}", e))?;
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("decryption failed: {}", e))?;

    String::from_utf8(plaintext).map_err(|e| format!("invalid UTF-8: {}", e))
}

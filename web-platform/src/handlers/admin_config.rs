use std::sync::atomic::Ordering;

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use crate::error::WebPlatformError;
use crate::models::ResponseData;
use crate::proxy::{filter_system_configs, is_network_proxy_key};
use crate::repository::SystemConfigRepository;
use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemConfigItem {
    pub key: String,
    pub value: String,
    pub description: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConfigRequest {
    pub configs: Vec<ConfigEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStats {
    pub total_projects: i64,
    pub running_services: i64,
    pub total_users: i64,
    pub global_concurrency_limit: i64,
    pub global_concurrency_used: i64,
}

/// GET /api/admin/config
pub async fn get_system_config(
    State(state): State<AppState>,
) -> Result<Json<ResponseData<Vec<SystemConfigItem>>>, WebPlatformError> {
    let configs = state.repo.list_system_configs().await?;
    Ok(Json(ResponseData::success(filter_system_configs(configs))))
}

/// PUT /api/admin/config
pub async fn update_system_config(
    State(state): State<AppState>,
    Json(req): Json<UpdateConfigRequest>,
) -> Result<Json<ResponseData<Vec<SystemConfigItem>>>, WebPlatformError> {
    if req.configs.is_empty() {
        return Err(WebPlatformError::BadRequest(
            "configs cannot be empty".to_string(),
        ));
    }

    for entry in &req.configs {
        if entry.key.is_empty() {
            return Err(WebPlatformError::BadRequest(
                "config key cannot be empty".to_string(),
            ));
        }
        if is_network_proxy_key(&entry.key) {
            return Err(WebPlatformError::BadRequest(
                "network_proxy.* keys must be managed through /api/admin/network-proxy".to_string(),
            ));
        }
        if entry.value.is_empty() {
            return Err(WebPlatformError::BadRequest(format!(
                "config value for '{}' cannot be empty",
                entry.key
            )));
        }
    }

    let pairs: Vec<(&str, &str)> = req
        .configs
        .iter()
        .map(|c| (c.key.as_str(), c.value.as_str()))
        .collect();

    state.repo.update_system_configs(&pairs).await?;

    let configs = state.repo.list_system_configs().await?;
    Ok(Json(ResponseData::success(filter_system_configs(configs))))
}

/// GET /api/admin/stats
pub async fn get_system_stats(
    State(state): State<AppState>,
) -> Result<Json<ResponseData<SystemStats>>, WebPlatformError> {
    let (total_projects, running_services, total_users) = state.repo.get_system_stats().await?;

    // Get global concurrency limit from system_configs
    let configs = state.repo.list_system_configs().await?;
    let global_concurrency_limit = configs
        .iter()
        .find(|c| c.key == "max_concurrent_codex")
        .and_then(|c| c.value.parse::<i64>().ok())
        .unwrap_or(5);

    // Get current concurrency usage from the concurrency manager
    let global_concurrency_used = state
        .concurrency_manager
        .global_active
        .load(Ordering::Relaxed);

    Ok(Json(ResponseData::success(SystemStats {
        total_projects,
        running_services,
        total_users,
        global_concurrency_limit,
        global_concurrency_used,
    })))
}

use std::collections::HashMap;

use axum::{extract::State, Json};
use serde::Serialize;

use crate::crypto;
use crate::error::WebPlatformError;
use crate::models::ResponseData;
use crate::proxy::{
    normalize_no_proxy, redact_proxy_url, validate_no_proxy, validate_proxy_url,
    EffectiveProxyConfig, NetworkProxyConfigResponse, ProxyMode, ProxySecret, ProxySecretDisplay,
    ProxySecretMutation, ProxyWarning, SecretUpdate, UpdateNetworkProxyRequest, ALL_SECRET_KEY,
    AUTO_BYPASS_LOCAL_KEY, HTTPS_SECRET_KEY, HTTP_SECRET_KEY, MODE_KEY, NO_PROXY_KEY, VERSION_KEY,
};
use crate::repository::{NetworkProxyRepository, SystemConfigRepository};
use crate::AppState;

/// GET /api/admin/network-proxy
pub async fn get_network_proxy(
    State(state): State<AppState>,
) -> Result<Json<ResponseData<NetworkProxyConfigResponse>>, WebPlatformError> {
    let response = load_network_proxy_response(&state).await?;
    Ok(Json(ResponseData::success(response)))
}

/// GET /api/admin/network-proxy/effective
pub async fn get_effective_network_proxy(
    State(state): State<AppState>,
) -> Result<Json<ResponseData<serde_json::Value>>, WebPlatformError> {
    let loaded =
        load_effective_proxy_config_with_warnings(&state.repo, &state.encryption_key).await?;
    let effective = loaded.config;
    Ok(Json(ResponseData::success(serde_json::json!({
        "mode": effective.mode,
        "version": effective.version,
        "source": effective.source,
        "environment": effective.proxy_env_summary(),
        "warnings": loaded.warnings,
    }))))
}

/// POST /api/admin/network-proxy/test
pub async fn test_network_proxy(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<ResponseData<ProxyTestResult>>, WebPlatformError> {
    let object = body.as_object().ok_or_else(|| {
        WebPlatformError::BadRequest("request body must be a JSON object".to_string())
    })?;
    if object.contains_key("targetUrl") || object.contains_key("customUrl") {
        return Ok(Json(ResponseData::success(
            ProxyTestResult::validation_failed("custom test URLs are not allowed"),
        )));
    }
    let target_id = object
        .get("targetId")
        .and_then(|value| value.as_str())
        .ok_or_else(|| WebPlatformError::BadRequest("targetId is required".to_string()))?;
    let use_draft_config = object
        .get("useDraftConfig")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if !use_draft_config && object.contains_key("draftConfig") {
        return Ok(Json(ResponseData::success(
            ProxyTestResult::validation_failed(
                "draftConfig is only allowed when useDraftConfig is true",
            ),
        )));
    }

    let target = match target_id {
        "github" => ProxyTestTarget {
            host: "api.github.com",
            url: "https://api.github.com/",
            kind: ProxyTargetKind::Generic,
        },
        "gitlab" => ProxyTestTarget {
            host: "gitlab.com",
            url: "https://gitlab.com/",
            kind: ProxyTargetKind::Generic,
        },
        "linear" => ProxyTestTarget {
            host: "api.linear.app",
            url: "https://api.linear.app/graphql",
            kind: ProxyTargetKind::Generic,
        },
        "openai" => ProxyTestTarget {
            host: "api.openai.com",
            url: "https://api.openai.com/v1/models",
            kind: ProxyTargetKind::AuthenticatedApi,
        },
        _ => {
            return Ok(Json(ResponseData::success(
                ProxyTestResult::validation_failed("unknown targetId"),
            )))
        }
    };

    let effective = if use_draft_config {
        let Some(draft_config) = object.get("draftConfig") else {
            return Ok(Json(ResponseData::success(
                ProxyTestResult::validation_failed("draftConfig is required"),
            )));
        };
        let draft = match serde_json::from_value::<UpdateNetworkProxyRequest>(draft_config.clone())
        {
            Ok(draft) => draft,
            Err(error) => {
                return Ok(Json(ResponseData::success(
                    ProxyTestResult::validation_failed(&format!("invalid draftConfig: {error}")),
                )))
            }
        };
        match build_effective_proxy_config_from_request(&state, &draft).await {
            Ok(config) => config,
            Err(error) => {
                return Ok(Json(ResponseData::success(
                    ProxyTestResult::validation_failed(&error.to_string()),
                )))
            }
        }
    } else {
        let loaded =
            load_effective_proxy_config_with_warnings(&state.repo, &state.encryption_key).await?;
        if loaded.warnings.iter().any(|warning| warning.blocking) {
            return Ok(Json(ResponseData::success(
                ProxyTestResult::validation_failed("proxy configuration is invalid"),
            )));
        }
        loaded.config
    };
    let started = std::time::Instant::now();
    let client = effective
        .apply_to_reqwest_builder(
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .user_agent("Symphony-WebPlatform/0.3.0")
                .redirect(reqwest::redirect::Policy::none()),
        )?
        .build()?;
    let result = match client.head(target.url).send().await {
        Ok(response) => {
            let (message, reachable) = classify_target_status(response.status(), target.kind);
            if reachable {
                ProxyTestResult::success_with_message(
                    target.host,
                    &effective,
                    started.elapsed().as_millis(),
                    message,
                )
            } else {
                ProxyTestResult::target_failed(
                    target.host,
                    &effective,
                    started.elapsed().as_millis(),
                    &format!("target returned HTTP status {}", response.status().as_u16()),
                )
            }
        }
        Err(error) if error.is_timeout() => ProxyTestResult::timeout(
            target.host,
            &effective,
            started.elapsed().as_millis(),
            "request timed out",
        ),
        Err(error) if error.is_connect() => ProxyTestResult::proxy_failed(
            target.host,
            &effective,
            started.elapsed().as_millis(),
            "proxy or target connection failed",
        ),
        Err(error) => ProxyTestResult::target_failed(
            target.host,
            &effective,
            started.elapsed().as_millis(),
            &error.to_string(),
        ),
    };
    Ok(Json(ResponseData::success(result)))
}

/// PUT /api/admin/network-proxy
pub async fn update_network_proxy(
    State(state): State<AppState>,
    Json(req): Json<UpdateNetworkProxyRequest>,
) -> Result<Json<ResponseData<NetworkProxyConfigResponse>>, WebPlatformError> {
    validate_no_proxy(&req.no_proxy)?;

    let configs = config_map(state.repo.list_system_configs().await?);
    let current_version = config_value(&configs, VERSION_KEY, "1");
    let current_mode = ProxyMode::parse(config_value(&configs, MODE_KEY, "inherit_env"))
        .unwrap_or(ProxyMode::Disabled);
    let current_no_proxy = config_value(&configs, NO_PROXY_KEY, "").to_string();
    let current_auto_bypass_local = config_value(&configs, AUTO_BYPASS_LOCAL_KEY, "true") == "true";
    if req.expected_version != current_version {
        return Err(WebPlatformError::Conflict(
            "network proxy config version conflict".to_string(),
        ));
    }

    let current = CurrentSecrets {
        http: state.repo.get_proxy_secret(HTTP_SECRET_KEY).await?,
        https: state.repo.get_proxy_secret(HTTPS_SECRET_KEY).await?,
        all: state.repo.get_proxy_secret(ALL_SECRET_KEY).await?,
    };

    let http_value = validate_secret_action(&req.http_proxy, current.http.as_ref(), "httpProxy")?;
    let https_value =
        validate_secret_action(&req.https_proxy, current.https.as_ref(), "httpsProxy")?;
    let all_value = validate_secret_action(&req.all_proxy, current.all.as_ref(), "allProxy")?;

    if req.mode == ProxyMode::Manual
        && http_value.is_none()
        && https_value.is_none()
        && all_value.is_none()
    {
        return Err(WebPlatformError::BadRequest(
            "manual proxy mode requires at least one proxy url".to_string(),
        ));
    }

    if !network_proxy_request_has_changes(
        &req,
        current_mode,
        &current_no_proxy,
        current_auto_bypass_local,
        &current,
        &state.encryption_key,
    )? {
        let response = load_network_proxy_response(&state).await?;
        return Ok(Json(ResponseData::success(response)));
    }

    let mut secret_mutations = Vec::new();
    collect_secret_mutation(
        &mut secret_mutations,
        HTTP_SECRET_KEY,
        "network_proxy_http",
        &req.http_proxy,
        &state.encryption_key,
    )?;
    collect_secret_mutation(
        &mut secret_mutations,
        HTTPS_SECRET_KEY,
        "network_proxy_https",
        &req.https_proxy,
        &state.encryption_key,
    )?;
    collect_secret_mutation(
        &mut secret_mutations,
        ALL_SECRET_KEY,
        "network_proxy_all",
        &req.all_proxy,
        &state.encryption_key,
    )?;

    let next_version = current_version
        .parse::<u64>()
        .unwrap_or(1)
        .saturating_add(1)
        .to_string();
    state
        .repo
        .update_network_proxy_config(
            &req.expected_version,
            vec![
                (MODE_KEY.to_string(), req.mode.as_str().to_string()),
                (NO_PROXY_KEY.to_string(), req.no_proxy.clone()),
                (
                    AUTO_BYPASS_LOCAL_KEY.to_string(),
                    if req.auto_bypass_local {
                        "true".to_string()
                    } else {
                        "false".to_string()
                    },
                ),
                (VERSION_KEY.to_string(), next_version),
            ],
            secret_mutations,
        )
        .await?;

    if let Some(alert_manager) = &state.alert_manager {
        if let Err(error) = alert_manager
            .reload_channels(&state.repo, &state.encryption_key)
            .await
        {
            tracing::warn!(error = %error, "failed to rebuild notification clients after proxy update");
        }
    }

    let response = load_network_proxy_response(&state).await?;
    Ok(Json(ResponseData::success(response)))
}

struct ProxyTestTarget {
    host: &'static str,
    url: &'static str,
    kind: ProxyTargetKind,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTestResult {
    pub status: String,
    pub target_host: String,
    pub proxy_used: bool,
    pub proxy_summary: String,
    pub duration_ms: u128,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProxyTargetKind {
    Generic,
    AuthenticatedApi,
}

fn classify_target_status(
    status: reqwest::StatusCode,
    kind: ProxyTargetKind,
) -> (&'static str, bool) {
    if status.is_success() || status.is_redirection() {
        return ("connection succeeded", true);
    }
    if kind == ProxyTargetKind::AuthenticatedApi && status == reqwest::StatusCode::UNAUTHORIZED {
        return ("target is reachable and requires authentication", true);
    }
    ("target returned non-success status", false)
}

impl ProxyTestResult {
    fn validation_failed(message: &str) -> Self {
        Self {
            status: "validation_failed".to_string(),
            target_host: String::new(),
            proxy_used: false,
            proxy_summary: String::new(),
            duration_ms: 0,
            message: message.to_string(),
        }
    }

    fn success_with_message(
        target_host: &str,
        effective: &EffectiveProxyConfig,
        duration_ms: u128,
        message: &str,
    ) -> Self {
        Self::with_status("success", target_host, effective, duration_ms, message)
    }

    fn proxy_failed(
        target_host: &str,
        effective: &EffectiveProxyConfig,
        duration_ms: u128,
        message: &str,
    ) -> Self {
        Self::with_status("proxy_failed", target_host, effective, duration_ms, message)
    }

    fn target_failed(
        target_host: &str,
        effective: &EffectiveProxyConfig,
        duration_ms: u128,
        message: &str,
    ) -> Self {
        Self::with_status(
            "target_failed",
            target_host,
            effective,
            duration_ms,
            message,
        )
    }

    fn timeout(
        target_host: &str,
        effective: &EffectiveProxyConfig,
        duration_ms: u128,
        message: &str,
    ) -> Self {
        Self::with_status("timeout", target_host, effective, duration_ms, message)
    }

    fn with_status(
        status: &str,
        target_host: &str,
        effective: &EffectiveProxyConfig,
        duration_ms: u128,
        message: &str,
    ) -> Self {
        let proxy_used = effective.mode != ProxyMode::Disabled
            && (effective.http_proxy.is_some()
                || effective.https_proxy.is_some()
                || effective.all_proxy.is_some());
        let proxy_summary = effective
            .proxy_env_summary()
            .into_iter()
            .map(|(key, value)| format!("{}={}", key, value))
            .collect::<Vec<_>>()
            .join(", ");
        Self {
            status: status.to_string(),
            target_host: target_host.to_string(),
            proxy_used,
            proxy_summary,
            duration_ms,
            message: message.to_string(),
        }
    }
}

pub async fn load_effective_proxy_config(
    repo: &crate::repository::SqliteRepository,
    encryption_key: &[u8; 32],
) -> Result<EffectiveProxyConfig, WebPlatformError> {
    Ok(
        load_effective_proxy_config_with_warnings(repo, encryption_key)
            .await?
            .config,
    )
}

pub struct EffectiveProxyConfigLoad {
    pub config: EffectiveProxyConfig,
    pub warnings: Vec<ProxyWarning>,
}

pub async fn load_effective_proxy_config_with_warnings(
    repo: &crate::repository::SqliteRepository,
    encryption_key: &[u8; 32],
) -> Result<EffectiveProxyConfigLoad, WebPlatformError> {
    let configs = config_map(repo.list_system_configs().await?);
    let version = config_value(&configs, VERSION_KEY, "1").to_string();
    let mode = ProxyMode::parse(config_value(&configs, MODE_KEY, "inherit_env"))
        .unwrap_or(ProxyMode::Disabled);
    let no_proxy = config_value(&configs, NO_PROXY_KEY, "").to_string();
    let auto_bypass_local = config_value(&configs, AUTO_BYPASS_LOCAL_KEY, "true") == "true";
    let warnings = Vec::new();

    match mode {
        ProxyMode::Disabled => Ok(EffectiveProxyConfigLoad {
            config: EffectiveProxyConfig::disabled(version, "system_config"),
            warnings,
        }),
        ProxyMode::InheritEnv => {
            let mut effective = EffectiveProxyConfig::from_env(version);
            effective.source = "environment".to_string();
            Ok(EffectiveProxyConfigLoad {
                config: effective,
                warnings,
            })
        }
        ProxyMode::Manual => {
            let http = decrypt_secret(
                repo.get_proxy_secret(HTTP_SECRET_KEY).await?,
                encryption_key,
                "network_proxy_http",
            );
            let https = decrypt_secret(
                repo.get_proxy_secret(HTTPS_SECRET_KEY).await?,
                encryption_key,
                "network_proxy_https",
            );
            let all = decrypt_secret(
                repo.get_proxy_secret(ALL_SECRET_KEY).await?,
                encryption_key,
                "network_proxy_all",
            );
            let (http, https, all) = match (http, https, all) {
                (Ok(http), Ok(https), Ok(all)) => (http, https, all),
                _ => {
                    return Ok(EffectiveProxyConfigLoad {
                        config: EffectiveProxyConfig::disabled(version, "fallback_disabled"),
                        warnings: vec![blocking_proxy_secret_warning()],
                    })
                }
            };
            if http.is_none() && https.is_none() && all.is_none() {
                return Ok(EffectiveProxyConfigLoad {
                    config: EffectiveProxyConfig::disabled(version, "fallback_disabled"),
                    warnings,
                });
            }
            Ok(EffectiveProxyConfigLoad {
                config: EffectiveProxyConfig {
                    mode,
                    version,
                    source: "system_config".to_string(),
                    http_proxy: http,
                    https_proxy: https,
                    all_proxy: all,
                    no_proxy: normalize_no_proxy(&no_proxy, auto_bypass_local),
                },
                warnings,
            })
        }
    }
}

async fn load_network_proxy_response(
    state: &AppState,
) -> Result<NetworkProxyConfigResponse, WebPlatformError> {
    let configs = config_map(state.repo.list_system_configs().await?);
    let version = config_value(&configs, VERSION_KEY, "1").to_string();
    let mode = ProxyMode::parse(config_value(&configs, MODE_KEY, "inherit_env"))
        .unwrap_or(ProxyMode::Disabled);
    let no_proxy = config_value(&configs, NO_PROXY_KEY, "").to_string();
    let auto_bypass_local = config_value(&configs, AUTO_BYPASS_LOCAL_KEY, "true") == "true";
    let updated_at = configs
        .get(VERSION_KEY)
        .map(|item| item.updated_at.clone())
        .or_else(|| configs.get(MODE_KEY).map(|item| item.updated_at.clone()));

    let mut warnings = Vec::new();
    let http = secret_display(
        state.repo.get_proxy_secret(HTTP_SECRET_KEY).await?,
        &state.encryption_key,
        &mut warnings,
    );
    let https = secret_display(
        state.repo.get_proxy_secret(HTTPS_SECRET_KEY).await?,
        &state.encryption_key,
        &mut warnings,
    );
    let all = secret_display(
        state.repo.get_proxy_secret(ALL_SECRET_KEY).await?,
        &state.encryption_key,
        &mut warnings,
    );

    let effective_mode = if warnings.iter().any(|warning| warning.blocking) {
        ProxyMode::Disabled
    } else {
        mode
    };

    let needs_restart_project_count = state
        .repo
        .count_running_services_with_stale_proxy_version(&version)
        .await?;

    Ok(NetworkProxyConfigResponse {
        mode: effective_mode,
        version,
        source: match effective_mode {
            ProxyMode::Disabled if warnings.iter().any(|warning| warning.blocking) => {
                "fallback_disabled".to_string()
            }
            ProxyMode::InheritEnv => "environment".to_string(),
            _ => "system_config".to_string(),
        },
        http_proxy: http,
        https_proxy: https,
        all_proxy: all,
        no_proxy,
        auto_bypass_local,
        needs_restart_project_count,
        updated_at,
        warnings,
    })
}

struct CurrentSecrets {
    http: Option<ProxySecret>,
    https: Option<ProxySecret>,
    all: Option<ProxySecret>,
}

fn network_proxy_request_has_changes(
    req: &UpdateNetworkProxyRequest,
    current_mode: ProxyMode,
    current_no_proxy: &str,
    current_auto_bypass_local: bool,
    current: &CurrentSecrets,
    encryption_key: &[u8; 32],
) -> Result<bool, WebPlatformError> {
    if req.mode != current_mode
        || req.no_proxy != current_no_proxy
        || req.auto_bypass_local != current_auto_bypass_local
    {
        return Ok(true);
    }

    Ok(secret_update_changes(
        &req.http_proxy,
        current.http.as_ref(),
        "network_proxy_http",
        encryption_key,
    )? || secret_update_changes(
        &req.https_proxy,
        current.https.as_ref(),
        "network_proxy_https",
        encryption_key,
    )? || secret_update_changes(
        &req.all_proxy,
        current.all.as_ref(),
        "network_proxy_all",
        encryption_key,
    )?)
}

fn secret_update_changes(
    update: &SecretUpdate,
    current: Option<&ProxySecret>,
    expected_kind: &str,
    encryption_key: &[u8; 32],
) -> Result<bool, WebPlatformError> {
    match update.action.as_str() {
        "keep" => Ok(false),
        "clear" => Ok(current.is_some()),
        "set" => {
            let next = update.value.as_deref().unwrap_or_default().trim();
            let Some(current) = current else {
                return Ok(true);
            };
            if current.kind != expected_kind {
                return Ok(true);
            }
            match crypto::decrypt(&current.encrypted_value, encryption_key) {
                Ok(value) => Ok(value != next),
                Err(_) => Ok(true),
            }
        }
        _ => unreachable!("validated before change detection"),
    }
}

fn validate_secret_action(
    update: &SecretUpdate,
    current: Option<&ProxySecret>,
    field: &str,
) -> Result<Option<()>, WebPlatformError> {
    match update.action.as_str() {
        "keep" => {
            if update.value.is_some() {
                return Err(WebPlatformError::BadRequest(format!(
                    "{} keep action must not include value",
                    field
                )));
            }
            if current.is_none() {
                return Err(WebPlatformError::BadRequest(format!(
                    "{} keep action requires an existing secret",
                    field
                )));
            }
            Ok(Some(()))
        }
        "set" => {
            let value = update.value.as_deref().ok_or_else(|| {
                WebPlatformError::BadRequest(format!("{} set action requires value", field))
            })?;
            if value.trim().is_empty() {
                return Err(WebPlatformError::BadRequest(format!(
                    "{} proxy url cannot be empty",
                    field
                )));
            }
            validate_proxy_url(value)?;
            Ok(Some(()))
        }
        "clear" => {
            if update.value.is_some() {
                return Err(WebPlatformError::BadRequest(format!(
                    "{} clear action must not include value",
                    field
                )));
            }
            Ok(None)
        }
        _ => Err(WebPlatformError::BadRequest(format!(
            "{} action must be keep, set or clear",
            field
        ))),
    }
}

async fn build_effective_proxy_config_from_request(
    state: &AppState,
    req: &UpdateNetworkProxyRequest,
) -> Result<EffectiveProxyConfig, WebPlatformError> {
    validate_no_proxy(&req.no_proxy)?;
    let current_version = state.repo.current_network_proxy_version().await?;
    let current = CurrentSecrets {
        http: state.repo.get_proxy_secret(HTTP_SECRET_KEY).await?,
        https: state.repo.get_proxy_secret(HTTPS_SECRET_KEY).await?,
        all: state.repo.get_proxy_secret(ALL_SECRET_KEY).await?,
    };

    let http = resolve_secret_update(
        &req.http_proxy,
        current.http.as_ref(),
        "httpProxy",
        "network_proxy_http",
        &state.encryption_key,
    )?;
    let https = resolve_secret_update(
        &req.https_proxy,
        current.https.as_ref(),
        "httpsProxy",
        "network_proxy_https",
        &state.encryption_key,
    )?;
    let all = resolve_secret_update(
        &req.all_proxy,
        current.all.as_ref(),
        "allProxy",
        "network_proxy_all",
        &state.encryption_key,
    )?;

    if req.mode == ProxyMode::Manual && http.is_none() && https.is_none() && all.is_none() {
        return Err(WebPlatformError::BadRequest(
            "manual proxy mode requires at least one proxy url".to_string(),
        ));
    }

    match req.mode {
        ProxyMode::Disabled => Ok(EffectiveProxyConfig::disabled(
            current_version,
            "draft_disabled",
        )),
        ProxyMode::InheritEnv => {
            let mut effective = EffectiveProxyConfig::from_env(current_version);
            effective.source = "draft_environment".to_string();
            Ok(effective)
        }
        ProxyMode::Manual => Ok(EffectiveProxyConfig {
            mode: ProxyMode::Manual,
            version: current_version,
            source: "draft".to_string(),
            http_proxy: http,
            https_proxy: https,
            all_proxy: all,
            no_proxy: normalize_no_proxy(&req.no_proxy, req.auto_bypass_local),
        }),
    }
}

fn resolve_secret_update(
    update: &SecretUpdate,
    current: Option<&ProxySecret>,
    field: &str,
    expected_kind: &str,
    encryption_key: &[u8; 32],
) -> Result<Option<String>, WebPlatformError> {
    match update.action.as_str() {
        "keep" => {
            validate_secret_action(update, current, field)?;
            let secret = current.cloned();
            decrypt_secret(secret, encryption_key, expected_kind)
        }
        "set" => {
            validate_secret_action(update, current, field)?;
            Ok(update
                .value
                .as_deref()
                .map(str::trim)
                .map(ToOwned::to_owned))
        }
        "clear" => {
            validate_secret_action(update, current, field)?;
            Ok(None)
        }
        _ => Err(WebPlatformError::BadRequest(format!(
            "{} action must be keep, set or clear",
            field
        ))),
    }
}

fn collect_secret_mutation(
    mutations: &mut Vec<ProxySecretMutation>,
    key: &str,
    kind: &str,
    update: &SecretUpdate,
    encryption_key: &[u8; 32],
) -> Result<(), WebPlatformError> {
    match update.action.as_str() {
        "set" => {
            let value = update.value.as_deref().unwrap();
            let encrypted = crypto::encrypt(value, encryption_key)?;
            mutations.push(ProxySecretMutation {
                key: key.to_string(),
                kind: kind.to_string(),
                encrypted_value: Some(encrypted),
            });
            Ok(())
        }
        "clear" => {
            mutations.push(ProxySecretMutation {
                key: key.to_string(),
                kind: kind.to_string(),
                encrypted_value: None,
            });
            Ok(())
        }
        "keep" => Ok(()),
        _ => unreachable!("validated before apply"),
    }
}

fn decrypt_secret(
    secret: Option<ProxySecret>,
    encryption_key: &[u8; 32],
    expected_kind: &str,
) -> Result<Option<String>, WebPlatformError> {
    match secret {
        Some(secret) if secret.kind == expected_kind => {
            crypto::decrypt(&secret.encrypted_value, encryption_key).map(Some)
        }
        Some(_) => Err(WebPlatformError::Internal(
            "proxy secret kind mismatch".to_string(),
        )),
        None => Ok(None),
    }
}

fn secret_display(
    secret: Option<ProxySecret>,
    encryption_key: &[u8; 32],
    warnings: &mut Vec<ProxyWarning>,
) -> ProxySecretDisplay {
    let Some(secret) = secret else {
        return ProxySecretDisplay {
            configured: false,
            display_value: String::new(),
            updated_at: None,
        };
    };

    let expected_kind = match secret.key.as_str() {
        HTTP_SECRET_KEY => "network_proxy_http",
        HTTPS_SECRET_KEY => "network_proxy_https",
        ALL_SECRET_KEY => "network_proxy_all",
        _ => "",
    };

    match (secret.kind == expected_kind)
        .then(|| crypto::decrypt(&secret.encrypted_value, encryption_key).ok())
        .flatten()
        .and_then(|value| redact_proxy_url(&value).ok())
    {
        Some(display_value) => ProxySecretDisplay {
            configured: true,
            display_value,
            updated_at: Some(secret.updated_at),
        },
        None => {
            warnings.push(blocking_proxy_secret_warning());
            ProxySecretDisplay {
                configured: true,
                display_value: String::new(),
                updated_at: Some(secret.updated_at),
            }
        }
    }
}

fn blocking_proxy_secret_warning() -> ProxyWarning {
    ProxyWarning {
        code: "proxy_secret_decrypt_failed".to_string(),
        severity: "error".to_string(),
        blocking: true,
        message: "代理配置不可用，已进入禁用态".to_string(),
    }
}

fn config_map(
    configs: Vec<crate::handlers::admin_config::SystemConfigItem>,
) -> HashMap<String, crate::handlers::admin_config::SystemConfigItem> {
    configs
        .into_iter()
        .map(|item| (item.key.clone(), item))
        .collect()
}

fn config_value<'a>(
    configs: &'a HashMap<String, crate::handlers::admin_config::SystemConfigItem>,
    key: &str,
    default: &'a str,
) -> &'a str {
    configs
        .get(key)
        .map(|item| item.value.as_str())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authenticated_api_unauthorized_counts_as_reachable() {
        let (message, reachable) = classify_target_status(
            reqwest::StatusCode::UNAUTHORIZED,
            ProxyTargetKind::AuthenticatedApi,
        );

        assert!(reachable);
        assert_eq!(message, "target is reachable and requires authentication");
    }

    #[test]
    fn generic_target_unauthorized_still_fails() {
        let (message, reachable) =
            classify_target_status(reqwest::StatusCode::UNAUTHORIZED, ProxyTargetKind::Generic);

        assert!(!reachable);
        assert_eq!(message, "target returned non-success status");
    }
}

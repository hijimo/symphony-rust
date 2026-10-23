use axum::{extract::State, Json};
use std::time::{Duration, Instant};

use crate::auth::jwt::Claims;
use crate::error::WebPlatformError;
use crate::handlers::network_proxy::load_effective_proxy_config;
use crate::models::concurrency::{ValidateTokenRequest, ValidateTokenResponse};
use crate::models::ResponseData;
use crate::repository::UserConfigRepository;
use crate::AppState;

/// POST /api/user/config/validate-token
///
/// Validates a platform token by calling the platform API.
/// Enforces a minimum 500ms response time to prevent timing attacks.
pub async fn validate_token(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Json(req): Json<ValidateTokenRequest>,
) -> Result<Json<ResponseData<ValidateTokenResponse>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id".to_string()))?;

    // Rate limit: 3/min/user
    if let Err(retry_after) = state
        .phase3_rate_limiter
        .check("validate_token", user_id, 3)
    {
        return Err(WebPlatformError::RateLimited(retry_after));
    }

    // Validate platform
    if req.platform != "gitlab" && req.platform != "github" {
        return Err(WebPlatformError::BadRequest(
            "platform must be 'gitlab' or 'github'".to_string(),
        ));
    }

    let start = Instant::now();

    // Determine host for GitLab
    let host = if req.platform == "gitlab" {
        let config = state.repo.get_config(user_id).await?;
        config
            .and_then(|c| c.gitlab_host)
            .unwrap_or_else(|| "https://gitlab.com".to_string())
    } else {
        "https://api.github.com".to_string()
    };

    // Call platform API to validate
    let proxy_config = load_effective_proxy_config(&state.repo, &state.encryption_key).await?;
    let result = validate_with_platform(&req.platform, &req.token, &host, &proxy_config).await;

    // Enforce minimum 500ms response time
    let elapsed = start.elapsed();
    if elapsed < Duration::from_millis(500) {
        tokio::time::sleep(Duration::from_millis(500) - elapsed).await;
    }

    let response = match result {
        Ok((username, scopes)) => ValidateTokenResponse {
            valid: true,
            username: Some(username),
            scopes,
            error: None,
        },
        Err(err) => ValidateTokenResponse {
            valid: false,
            username: None,
            scopes: vec![],
            error: Some(err),
        },
    };

    Ok(Json(ResponseData::success(response)))
}

async fn validate_with_platform(
    platform: &str,
    token: &str,
    host: &str,
    proxy_config: &crate::proxy::EffectiveProxyConfig,
) -> Result<(String, Vec<String>), String> {
    let client = proxy_config
        .apply_to_reqwest_builder(reqwest::Client::builder())
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;

    match platform {
        "gitlab" => {
            let url = format!("{}/api/v4/user", host.trim_end_matches('/'));
            let resp = client
                .get(&url)
                .header("PRIVATE-TOKEN", token)
                .timeout(Duration::from_secs(10))
                .send()
                .await
                .map_err(|e| format!("Network error: {}", e))?;

            if resp.status().is_success() {
                let body: serde_json::Value = resp
                    .json()
                    .await
                    .map_err(|e| format!("Parse error: {}", e))?;
                let username = body["username"].as_str().unwrap_or("unknown").to_string();
                Ok((username, vec!["api".to_string()]))
            } else if resp.status().as_u16() == 401 {
                Err("Token is invalid or expired".to_string())
            } else {
                Err(format!("Platform returned status {}", resp.status()))
            }
        }
        "github" => {
            let resp = client
                .get("https://api.github.com/user")
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "symphony-web-platform")
                .timeout(Duration::from_secs(10))
                .send()
                .await
                .map_err(|e| format!("Network error: {}", e))?;

            if resp.status().is_success() {
                let body: serde_json::Value = resp
                    .json()
                    .await
                    .map_err(|e| format!("Parse error: {}", e))?;
                let username = body["login"].as_str().unwrap_or("unknown").to_string();
                // Parse scopes from X-OAuth-Scopes header
                let scopes = vec!["repo".to_string()];
                Ok((username, scopes))
            } else if resp.status().as_u16() == 401 {
                Err("Token is invalid or expired".to_string())
            } else {
                Err(format!("Platform returned status {}", resp.status()))
            }
        }
        _ => Err("Unsupported platform".to_string()),
    }
}

/// GitLab access levels (subset).
pub const GITLAB_ACCESS_REPORTER: i64 = 20;
pub const GITLAB_ACCESS_DEVELOPER: i64 = 30;

/// Extract the effective access level from a GitLab `GET /projects/:id` body:
/// max of `permissions.project_access.access_level` and
/// `permissions.group_access.access_level`. Returns 0 when neither is present.
pub fn parse_gitlab_access_level(body: &serde_json::Value) -> i64 {
    let perms = &body["permissions"];
    ["project_access", "group_access"]
        .iter()
        .filter_map(|k| perms[*k]["access_level"].as_i64())
        .max()
        .unwrap_or(0)
}

/// Query the token owner's access level on `namespace/repo_name` via
/// `GET {platform_host}/api/v4/projects/:urlencoded_path`.
///
/// `platform_host` includes the scheme (e.g. `http://gitlab.example.com:8081`),
/// so tests can point it at a local mock server. Returns `Ok(0)` when the
/// repository is not visible to the token (404).
pub async fn fetch_gitlab_access_level(
    platform_host: &str,
    token: &str,
    project_path: &str,
    proxy_config: &crate::proxy::EffectiveProxyConfig,
) -> Result<i64, String> {
    let client = proxy_config
        .apply_to_reqwest_builder(reqwest::Client::builder())
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!(
        "{}/api/v4/projects/{}",
        platform_host.trim_end_matches('/'),
        urlencoding::encode(project_path)
    );
    let resp = client
        .get(&url)
        .header("PRIVATE-TOKEN", token)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    match resp.status().as_u16() {
        200..=299 => {
            let body: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("Parse error: {}", e))?;
            Ok(parse_gitlab_access_level(&body))
        }
        401 => Err("Token is invalid or expired".to_string()),
        403 | 404 => Ok(0),
        status => Err(format!("Platform returned status {}", status)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn access_level_takes_max_of_project_and_group() {
        let body = json!({
            "id": 1,
            "permissions": {
                "project_access": {"access_level": 20, "notification_level": 3},
                "group_access": {"access_level": 40, "notification_level": 3}
            }
        });
        assert_eq!(parse_gitlab_access_level(&body), 40);
    }

    #[test]
    fn access_level_handles_null_entries() {
        let body = json!({
            "permissions": {"project_access": null, "group_access": {"access_level": 30}}
        });
        assert_eq!(parse_gitlab_access_level(&body), 30);
        let body = json!({
            "permissions": {"project_access": {"access_level": 10}, "group_access": null}
        });
        assert_eq!(parse_gitlab_access_level(&body), 10);
    }

    #[test]
    fn access_level_defaults_to_zero() {
        assert_eq!(parse_gitlab_access_level(&json!({})), 0);
        assert_eq!(
            parse_gitlab_access_level(
                &json!({"permissions": {"project_access": null, "group_access": null}})
            ),
            0
        );
    }
}

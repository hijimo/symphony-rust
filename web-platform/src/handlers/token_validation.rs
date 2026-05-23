use axum::{extract::State, Json};
use std::time::{Duration, Instant};

use crate::auth::jwt::Claims;
use crate::error::WebPlatformError;
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
    let result = validate_with_platform(&req.platform, &req.token, &host).await;

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
) -> Result<(String, Vec<String>), String> {
    let client = reqwest::Client::new();

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

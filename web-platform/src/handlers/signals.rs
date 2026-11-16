//! External signals (docs/方案/ai-native-sdlc-p3-design.md §3 S1):
//! `POST /api/ingest/{project_id}/{source}` (no JWT) and the key /
//! signal management endpoints of a project.

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use hmac::{Hmac, KeyInit, Mac};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::auth::jwt::Claims;
use crate::error::WebPlatformError;
use crate::middleware::project_access::{require_project_member, require_project_owner};
use crate::models::ResponseData;
use crate::signals::sources::{self, ParseError};
use crate::signals::store::{IngestKey, Signal, Upsert};
use crate::signals::{MAX_PENDING_PER_PROJECT, PER_KEY_PER_MINUTE, SOURCES};
use crate::AppState;

fn user_id(claims: &Claims) -> Result<i64, WebPlatformError> {
    claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".into()))
}

fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

fn eq_ct(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[derive(Debug, Deserialize)]
pub struct NewKeyBody {
    pub source: String,
    pub name: String,
}

/// POST /api/projects/{id}/signal-keys — owner only; the secret is returned once.
pub async fn create_key(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(body): Json<NewKeyBody>,
) -> Result<Json<ResponseData<Value>>, WebPlatformError> {
    require_project_owner(&claims, project_id, &state.repo).await?;
    if !SOURCES.contains(&body.source.as_str()) {
        return Err(WebPlatformError::BadRequest(format!(
            "来源只能是 {}",
            SOURCES.join("、")
        )));
    }
    let name = body.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(WebPlatformError::BadRequest("名称长度需为 1-100".into()));
    }
    let mut raw = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut raw);
    let secret = format!("sig_{}", hex::encode(raw));
    let secret_enc = (body.source == "sentry")
        .then(|| crate::crypto::encrypt(&secret, &state.encryption_key))
        .transpose()?;
    let key = state
        .sdlc
        .signals
        .create_key(
            project_id,
            &body.source,
            name,
            &sha256_hex(&secret),
            secret_enc,
            user_id(&claims)?,
        )
        .await?;
    let mut v = serde_json::to_value(&key).unwrap_or_default();
    v["secret"] = json!(secret);
    Ok(Json(ResponseData::success(v)))
}

/// GET /api/projects/{id}/signal-keys — secrets are never returned.
pub async fn list_keys(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<ResponseData<Vec<IngestKey>>>, WebPlatformError> {
    require_project_member(&claims, project_id, &state.repo).await?;
    Ok(Json(ResponseData::success(
        state.sdlc.signals.list_keys(project_id).await?,
    )))
}

/// DELETE /api/projects/{id}/signal-keys/{key_id} — owner only.
pub async fn revoke_key(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, key_id)): Path<(i64, i64)>,
) -> Result<Json<ResponseData<()>>, WebPlatformError> {
    require_project_owner(&claims, project_id, &state.repo).await?;
    if !state.sdlc.signals.revoke_key(project_id, key_id).await? {
        return Err(WebPlatformError::NotFound("接入密钥不存在".into()));
    }
    Ok(Json(ResponseData::success(())))
}

#[derive(Debug, Deserialize)]
pub struct SignalsQuery {
    pub status: Option<String>,
}

/// GET /api/projects/{id}/signals?status=
pub async fn list_signals(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
    Query(q): Query<SignalsQuery>,
) -> Result<Json<ResponseData<Vec<Signal>>>, WebPlatformError> {
    require_project_member(&claims, project_id, &state.repo).await?;
    Ok(Json(ResponseData::success(
        state.sdlc.signals.list(project_id, q.status).await?,
    )))
}

/// The key that authenticates this request, if any.
async fn authenticate(
    state: &AppState,
    project_id: i64,
    source: &str,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Option<IngestKey>, WebPlatformError> {
    let keys = state.sdlc.signals.active_keys(project_id, source).await?;
    if source == "sentry" {
        let Some(sig) = headers
            .get("sentry-hook-signature")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| hex::decode(s.trim()).ok())
        else {
            return Ok(None);
        };
        for k in keys {
            let Some(enc) = k.secret_enc.as_deref() else {
                continue;
            };
            let Ok(secret) = crate::crypto::decrypt(enc, &state.encryption_key) else {
                continue;
            };
            let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
                continue;
            };
            mac.update(body);
            if mac.verify_slice(&sig).is_ok() {
                return Ok(Some(k));
            }
        }
        return Ok(None);
    }
    let Some(token) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return Ok(None);
    };
    let hash = sha256_hex(token.trim());
    Ok(keys
        .into_iter()
        .find(|k| eq_ct(k.key_hash.as_bytes(), hash.as_bytes())))
}

/// POST /api/ingest/{project_id}/{source}
pub async fn ingest(
    State(state): State<AppState>,
    Path((project_id, source)): Path<(i64, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<ResponseData<Value>>), WebPlatformError> {
    if !SOURCES.contains(&source.as_str()) {
        return Err(WebPlatformError::NotFound("unknown source".into()));
    }
    let key = authenticate(&state, project_id, &source, &headers, &body)
        .await?
        .ok_or(WebPlatformError::Unauthorized)?;
    if let Err(retry) = state
        .phase3_rate_limiter
        .check("signal_ingest", key.id, PER_KEY_PER_MINUTE)
    {
        return Err(WebPlatformError::RateLimited(retry));
    }
    state.sdlc.signals.touch_key(key.id).await?;
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|_| WebPlatformError::BadRequest("请求体不是合法的 JSON".into()))?;
    let parsed = match sources::parse(&source, &payload) {
        Ok(p) => p,
        Err(ParseError::Invalid(msg)) => return Err(WebPlatformError::BadRequest(msg)),
        Err(ParseError::Unsupported(s)) => {
            return Err(WebPlatformError::BadRequest(format!(
                "暂不支持解析 {s} 的请求"
            )))
        }
    };
    let (mut accepted, mut created) = (0, 0);
    for s in parsed {
        match state
            .sdlc
            .signals
            .upsert(project_id, &source, s, MAX_PENDING_PER_PROJECT)
            .await?
        {
            Upsert::Created => {
                accepted += 1;
                created += 1;
            }
            Upsert::Updated => accepted += 1,
            Upsert::Dropped => {
                tracing::warn!(project_id, %source, "too many untriaged signals; new signal dropped");
            }
        }
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(ResponseData::success(
            json!({"accepted": accepted, "created": created}),
        )),
    ))
}

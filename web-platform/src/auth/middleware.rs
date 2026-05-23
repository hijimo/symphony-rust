use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::auth::jwt::{verify_token, Claims};
use crate::error::WebPlatformError;
use crate::AppState;

pub async fn jwt_auth(
    state: axum::extract::State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(h) if h.starts_with("Bearer ") => &h[7..],
        _ => {
            return WebPlatformError::Unauthorized.into_response();
        }
    };

    match verify_token(token, &state.jwt_secret, &state.token_blacklist) {
        Ok(claims) => {
            req.extensions_mut().insert(claims);
            next.run(req).await
        }
        Err(e) => e.into_response(),
    }
}

pub async fn require_admin(req: Request, next: Next) -> Response {
    let claims = req.extensions().get::<Claims>();
    match claims {
        Some(c) if c.role == "admin" => next.run(req).await,
        _ => WebPlatformError::Forbidden.into_response(),
    }
}

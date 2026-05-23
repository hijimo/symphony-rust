use chrono::{Duration, Utc};
use dashmap::DashMap;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::error::WebPlatformError;
use crate::repository::TokenBlacklistRepository;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub username: String,
    pub role: String,
    pub iat: i64,
    pub exp: i64,
}

pub fn generate_token(
    user_id: i64,
    username: &str,
    role: &str,
    secret: &str,
) -> Result<(String, chrono::DateTime<Utc>), WebPlatformError> {
    let now = Utc::now();
    let expires_at = now + Duration::days(7);

    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        role: role.to_string(),
        iat: now.timestamp(),
        exp: expires_at.timestamp(),
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| WebPlatformError::Internal(format!("failed to generate token: {}", e)))?;

    Ok((token, expires_at))
}

pub fn verify_token(
    token: &str,
    secret: &str,
    blacklist: &Arc<DashMap<i64, chrono::DateTime<Utc>>>,
) -> Result<Claims, WebPlatformError> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|e| match e.kind() {
        jsonwebtoken::errors::ErrorKind::ExpiredSignature => WebPlatformError::Unauthorized,
        _ => WebPlatformError::Unauthorized,
    })?;

    let claims = token_data.claims;
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Unauthorized)?;

    if let Some(entry) = blacklist.get(&user_id) {
        let invalidated_at = *entry.value();
        if claims.iat < invalidated_at.timestamp() {
            return Err(WebPlatformError::Unauthorized);
        }
    }

    Ok(claims)
}

pub async fn invalidate_user_tokens(
    user_id: i64,
    blacklist: &Arc<DashMap<i64, chrono::DateTime<Utc>>>,
    repo: &impl TokenBlacklistRepository,
) {
    blacklist.insert(user_id, Utc::now());
    let _ = repo.add_to_blacklist(user_id, "token_invalidated").await;
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-key-at-least-32-chars-long!!";

    fn empty_blacklist() -> Arc<DashMap<i64, chrono::DateTime<Utc>>> {
        Arc::new(DashMap::new())
    }

    #[test]
    fn test_generate_token_contains_correct_claims() {
        let (token, _expires_at) = generate_token(42, "testuser", "admin", TEST_SECRET).unwrap();
        assert!(!token.is_empty());

        let blacklist = empty_blacklist();
        let claims = verify_token(&token, TEST_SECRET, &blacklist).unwrap();
        assert_eq!(claims.sub, "42");
        assert_eq!(claims.username, "testuser");
        assert_eq!(claims.role, "admin");
        assert!(claims.exp > claims.iat);
    }

    #[test]
    fn test_verify_valid_token_succeeds() {
        let (token, _) = generate_token(1, "admin", "admin", TEST_SECRET).unwrap();
        let blacklist = empty_blacklist();
        let result = verify_token(&token, TEST_SECRET, &blacklist);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_expired_token_fails() {
        let now = Utc::now();
        let claims = Claims {
            sub: "1".to_string(),
            username: "admin".to_string(),
            role: "admin".to_string(),
            iat: (now - Duration::hours(2)).timestamp(),
            exp: (now - Duration::hours(1)).timestamp(),
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(TEST_SECRET.as_bytes()),
        )
        .unwrap();

        let blacklist = empty_blacklist();
        let result = verify_token(&token, TEST_SECRET, &blacklist);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_invalid_signature_fails() {
        let (token, _) = generate_token(1, "admin", "admin", TEST_SECRET).unwrap();
        let blacklist = empty_blacklist();
        let result = verify_token(
            &token,
            "different-secret-key-at-least-32-chars!!",
            &blacklist,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_malformed_token_fails() {
        let blacklist = empty_blacklist();
        let result = verify_token("not.a.valid.jwt", TEST_SECRET, &blacklist);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_token_checks_blacklist() {
        let blacklist = empty_blacklist();

        // Generate a token with iat in the past
        let past = Utc::now() - Duration::seconds(10);
        let claims = Claims {
            sub: "1".to_string(),
            username: "admin".to_string(),
            role: "admin".to_string(),
            iat: past.timestamp(),
            exp: (Utc::now() + Duration::hours(1)).timestamp(),
        };
        let old_token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(TEST_SECRET.as_bytes()),
        )
        .unwrap();

        // Token should verify fine before blacklisting
        let result = verify_token(&old_token, TEST_SECRET, &blacklist);
        assert!(result.is_ok());

        // Blacklist user (invalidated_at = now, which is after the old token's iat)
        blacklist.insert(1, Utc::now());

        // Old token (iat in the past) should now fail
        let result_old = verify_token(&old_token, TEST_SECRET, &blacklist);
        assert!(result_old.is_err());

        // New token (iat = now) should still work since iat >= invalidated_at
        let (new_token, _) = generate_token(1, "admin", "admin", TEST_SECRET).unwrap();
        let result_new = verify_token(&new_token, TEST_SECRET, &blacklist);
        assert!(result_new.is_ok());
    }

    #[test]
    fn test_generate_token_expiry_is_7_days() {
        let before = Utc::now();
        let (_token, expires_at) = generate_token(1, "admin", "admin", TEST_SECRET).unwrap();
        let after = Utc::now();

        let expected_min = before + Duration::days(7);
        let expected_max = after + Duration::days(7);
        assert!(expires_at >= expected_min);
        assert!(expires_at <= expected_max);
    }
}

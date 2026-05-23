use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

use crate::error::WebPlatformError;

pub fn hash_password(password: &str) -> Result<String, WebPlatformError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| WebPlatformError::Internal(format!("failed to hash password: {}", e)))?;
    Ok(hash.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool, WebPlatformError> {
    let parsed_hash = PasswordHash::new(hash)
        .map_err(|e| WebPlatformError::Internal(format!("invalid password hash: {}", e)))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_password_produces_valid_argon2_hash() {
        let hash = hash_password("Admin@123456").unwrap();
        assert!(hash.starts_with("$argon2"));
        assert!(hash.len() > 50);
    }

    #[test]
    fn test_verify_password_correct_returns_true() {
        let hash = hash_password("Admin@123456").unwrap();
        let result = verify_password("Admin@123456", &hash).unwrap();
        assert!(result);
    }

    #[test]
    fn test_verify_password_incorrect_returns_false() {
        let hash = hash_password("Admin@123456").unwrap();
        let result = verify_password("WrongPassword", &hash).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_hash_password_different_salts() {
        let hash1 = hash_password("SamePassword").unwrap();
        let hash2 = hash_password("SamePassword").unwrap();
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_hash_password_empty_string() {
        let hash = hash_password("").unwrap();
        assert!(hash.starts_with("$argon2"));
        let result = verify_password("", &hash).unwrap();
        assert!(result);
    }

    #[test]
    fn test_verify_password_invalid_hash_format() {
        let result = verify_password("password", "not-a-valid-hash");
        assert!(result.is_err());
    }
}

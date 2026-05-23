use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    AeadCore, Aes256Gcm, Key, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

use crate::error::WebPlatformError;

pub fn encrypt(plaintext: &str, key: &[u8; 32]) -> Result<String, WebPlatformError> {
    let key = Key::<Aes256Gcm>::from_slice(key);
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|e| WebPlatformError::Internal(format!("encryption failed: {}", e)))?;

    let mut combined = nonce.to_vec();
    combined.extend_from_slice(&ciphertext);
    Ok(BASE64.encode(&combined))
}

pub fn decrypt(encoded: &str, key: &[u8; 32]) -> Result<String, WebPlatformError> {
    let combined = BASE64
        .decode(encoded)
        .map_err(|_| WebPlatformError::Internal("invalid base64 in encrypted value".to_string()))?;

    if combined.len() < 12 {
        return Err(WebPlatformError::Internal(
            "invalid ciphertext: too short".to_string(),
        ));
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let key = Key::<Aes256Gcm>::from_slice(key);
    let cipher = Aes256Gcm::new(key);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| WebPlatformError::Internal(format!("decryption failed: {}", e)))?;

    String::from_utf8(plaintext)
        .map_err(|_| WebPlatformError::Internal("invalid utf8 after decryption".to_string()))
}

pub fn parse_base64_key(b64: &str) -> Result<[u8; 32], WebPlatformError> {
    let bytes = BASE64.decode(b64).map_err(|_| {
        WebPlatformError::Internal("ENCRYPTION_KEY must be valid base64".to_string())
    })?;
    if bytes.len() != 32 {
        return Err(WebPlatformError::Internal(format!(
            "ENCRYPTION_KEY must decode to 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> [u8; 32] {
        [0x42u8; 32]
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = test_key();
        let plaintext = "hello world secret token";
        let encrypted = encrypt(plaintext, &key).unwrap();
        let decrypted = decrypt(&encrypted, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_different_inputs_produce_different_ciphertext() {
        let key = test_key();
        let enc1 = encrypt("message_one", &key).unwrap();
        let enc2 = encrypt("message_two", &key).unwrap();
        assert_ne!(enc1, enc2);
    }

    #[test]
    fn test_same_input_produces_different_ciphertext_due_to_nonce() {
        let key = test_key();
        let enc1 = encrypt("same_message", &key).unwrap();
        let enc2 = encrypt("same_message", &key).unwrap();
        assert_ne!(enc1, enc2);
    }

    #[test]
    fn test_wrong_key_decrypt_fails() {
        let key1 = [0x42u8; 32];
        let key2 = [0x43u8; 32];
        let encrypted = encrypt("secret", &key1).unwrap();
        let result = decrypt(&encrypted, &key2);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_string_encrypt_decrypt() {
        let key = test_key();
        let encrypted = encrypt("", &key).unwrap();
        let decrypted = decrypt(&encrypted, &key).unwrap();
        assert_eq!(decrypted, "");
    }

    #[test]
    fn test_decrypt_invalid_base64_fails() {
        let key = test_key();
        let result = decrypt("not-valid-base64!!!", &key);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_too_short_ciphertext_fails() {
        let key = test_key();
        let short = BASE64.encode([0u8; 5]);
        let result = decrypt(&short, &key);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_base64_key_valid() {
        let raw_key = [0xABu8; 32];
        let b64 = BASE64.encode(raw_key);
        let parsed = parse_base64_key(&b64).unwrap();
        assert_eq!(parsed, raw_key);
    }

    #[test]
    fn test_parse_base64_key_wrong_length() {
        let raw_key = [0xABu8; 16];
        let b64 = BASE64.encode(raw_key);
        let result = parse_base64_key(&b64);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_base64_key_invalid_base64() {
        let result = parse_base64_key("not-valid-base64!!!");
        assert!(result.is_err());
    }
}

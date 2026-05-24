use dashmap::DashMap;
use std::time::Instant;

use crate::error::WebPlatformError;

pub struct RateLimiter {
    entries: DashMap<String, (u32, Instant)>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
        }
    }

    pub fn check_rate_limit(&self, username: &str, ip: &str) -> Result<(), WebPlatformError> {
        let now = Instant::now();
        let window = std::time::Duration::from_secs(60);

        if self.entries.len() > 10000 {
            self.cleanup_expired(window);
        }

        let user_key = format!("user:{}", username);
        if let Some(mut entry) = self.entries.get_mut(&user_key) {
            let (count, start) = entry.value_mut();
            if now.duration_since(*start) > window {
                *count = 1;
                *start = now;
            } else if *count >= 5 {
                return Err(WebPlatformError::BadRequest(
                    "too many login attempts for this user, please try again later".to_string(),
                ));
            } else {
                *count += 1;
            }
        } else {
            self.entries.insert(user_key, (1, now));
        }

        let ip_key = format!("ip:{}", ip);
        if let Some(mut entry) = self.entries.get_mut(&ip_key) {
            let (count, start) = entry.value_mut();
            if now.duration_since(*start) > window {
                *count = 1;
                *start = now;
            } else if *count >= 20 {
                return Err(WebPlatformError::BadRequest(
                    "too many login attempts from this IP, please try again later".to_string(),
                ));
            } else {
                *count += 1;
            }
        } else {
            self.entries.insert(ip_key, (1, now));
        }

        Ok(())
    }

    fn cleanup_expired(&self, window: std::time::Duration) {
        let now = Instant::now();
        self.entries
            .retain(|_, (_, start)| now.duration_since(*start) <= window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allows_requests_under_threshold() {
        let limiter = RateLimiter::new();
        for _ in 0..4 {
            assert!(limiter.check_rate_limit("user1", "1.2.3.4").is_ok());
        }
    }

    #[test]
    fn test_blocks_at_username_threshold() {
        let limiter = RateLimiter::new();
        for _ in 0..5 {
            let _ = limiter.check_rate_limit("user1", "1.2.3.4");
        }
        let result = limiter.check_rate_limit("user1", "1.2.3.4");
        assert!(result.is_err());
    }

    #[test]
    fn test_blocks_at_ip_threshold() {
        let limiter = RateLimiter::new();
        for i in 0..20 {
            let username = format!("user{}", i);
            let _ = limiter.check_rate_limit(&username, "1.2.3.4");
        }
        let result = limiter.check_rate_limit("user_new", "1.2.3.4");
        assert!(result.is_err());
    }

    #[test]
    fn test_username_and_ip_independent() {
        let limiter = RateLimiter::new();
        for _ in 0..4 {
            assert!(limiter.check_rate_limit("user_a", "10.0.0.1").is_ok());
        }
        for _ in 0..4 {
            assert!(limiter.check_rate_limit("user_b", "10.0.0.2").is_ok());
        }
    }

    #[test]
    fn test_window_expiry_resets_count() {
        let limiter = RateLimiter::new();

        for _ in 0..5 {
            let _ = limiter.check_rate_limit("user1", "1.2.3.4");
        }
        assert!(limiter.check_rate_limit("user1", "1.2.3.4").is_err());

        let user_key = "user:user1".to_string();
        if let Some(mut entry) = limiter.entries.get_mut(&user_key) {
            let (_, start) = entry.value_mut();
            *start = Instant::now() - std::time::Duration::from_secs(61);
        }

        assert!(limiter.check_rate_limit("user1", "1.2.3.4").is_ok());
    }

    #[test]
    fn test_different_usernames_independent_counts() {
        let limiter = RateLimiter::new();
        for _ in 0..5 {
            let _ = limiter.check_rate_limit("user_a", "10.0.0.1");
        }
        assert!(limiter.check_rate_limit("user_a", "10.0.0.1").is_err());
        assert!(limiter.check_rate_limit("user_b", "10.0.0.2").is_ok());
    }
}

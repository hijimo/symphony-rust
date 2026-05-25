use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// A cached entry with TTL tracking.
#[derive(Clone)]
struct CacheEntry {
    value: String,
    created_at: Instant,
    ttl: Duration,
}

impl CacheEntry {
    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.ttl
    }
}

/// Singleflight + TTL cache for external API responses.
///
/// Provides:
/// - TTL-based expiration
/// - Singleflight deduplication (concurrent requests for the same key
///   only trigger one actual fetch)
/// - Empty result caching with reduced TTL
pub struct ApiCache {
    entries: DashMap<String, CacheEntry>,
    /// In-flight requests: key -> broadcast sender.
    /// When a request is in-flight, subsequent requests subscribe to the sender
    /// and wait for the result instead of making a duplicate call.
    in_flight: DashMap<String, Arc<broadcast::Sender<Result<String, String>>>>,
    pub default_ttl: Duration,
    pub empty_ttl: Duration,
    pub max_entries: usize,
}

impl ApiCache {
    pub fn new(default_ttl_secs: u64, empty_ttl_secs: u64, max_entries: usize) -> Self {
        Self {
            entries: DashMap::new(),
            in_flight: DashMap::new(),
            default_ttl: Duration::from_secs(default_ttl_secs),
            empty_ttl: Duration::from_secs(empty_ttl_secs),
            max_entries,
        }
    }

    /// Get a cached value if it exists and is not expired.
    pub fn get(&self, key: &str) -> Option<String> {
        if let Some(entry) = self.entries.get(key) {
            if !entry.is_expired() {
                return Some(entry.value.clone());
            }
            // Remove expired entry
            drop(entry);
            self.entries.remove(key);
        }
        None
    }

    /// Store a value in the cache.
    pub fn set(&self, key: String, value: String, is_empty: bool) {
        // Evict if over capacity
        if self.entries.len() >= self.max_entries {
            self.cleanup_expired();
        }

        let ttl = if is_empty {
            self.empty_ttl
        } else {
            self.default_ttl
        };

        self.entries.insert(
            key,
            CacheEntry {
                value,
                created_at: Instant::now(),
                ttl,
            },
        );
    }

    /// Store a value with a custom TTL.
    pub fn set_with_ttl(&self, key: String, value: String, ttl: Duration) {
        if self.entries.len() >= self.max_entries {
            self.cleanup_expired();
        }

        self.entries.insert(
            key,
            CacheEntry {
                value,
                created_at: Instant::now(),
                ttl,
            },
        );
    }

    /// Invalidate all cache entries matching a prefix.
    pub fn invalidate_prefix(&self, prefix: &str) {
        self.entries.retain(|k, _| !k.starts_with(prefix));
    }

    /// Invalidate all entries scoped to a project.
    ///
    /// Current API cache keys use `{user_id}:{project_id}:...`. This gives MR
    /// creation a project-level invalidation path without relying on a single
    /// user prefix.
    pub fn invalidate_project(&self, project_id: i64) {
        let project_id = project_id.to_string();
        self.entries
            .retain(|k, _| k.split(':').nth(1) != Some(project_id.as_str()));
    }

    /// Invalidate a specific cache key.
    pub fn invalidate(&self, key: &str) {
        self.entries.remove(key);
    }

    /// Check if a singleflight request is in progress for this key.
    /// Returns a receiver if one is in progress, or None if we should start one.
    pub fn try_join_inflight(
        &self,
        key: &str,
    ) -> Option<broadcast::Receiver<Result<String, String>>> {
        self.in_flight.get(key).map(|tx| tx.subscribe())
    }

    /// Register that we are starting a fetch for this key.
    /// Returns the sender to broadcast the result when done.
    pub fn start_inflight(&self, key: &str) -> Arc<broadcast::Sender<Result<String, String>>> {
        let (tx, _) = broadcast::channel(1);
        let tx = Arc::new(tx);
        self.in_flight.insert(key.to_string(), tx.clone());
        tx
    }

    /// Complete an in-flight request: broadcast result and remove from in_flight map.
    pub fn complete_inflight(&self, key: &str, result: Result<String, String>) {
        if let Some((_, tx)) = self.in_flight.remove(key) {
            let _ = tx.send(result);
        }
    }

    /// Remove expired entries.
    pub fn cleanup_expired(&self) {
        self.entries.retain(|_, entry| !entry.is_expired());
    }

    /// Get the creation time of a cached entry (for cached_at field).
    pub fn get_cached_at(&self, key: &str) -> Option<Instant> {
        self.entries.get(key).map(|e| e.created_at)
    }
}

impl Default for ApiCache {
    fn default() -> Self {
        Self::new(10, 3, 10000)
    }
}

#[cfg(test)]
mod tests {
    use super::ApiCache;

    #[test]
    fn invalidate_project_only_matches_project_id_segment() {
        let cache = ApiCache::new(60, 3, 100);
        cache.set(
            "10:2:kanban:abc".to_string(),
            "project-2".to_string(),
            false,
        );
        cache.set(
            "10:12:issue:2:mrs".to_string(),
            "project-12".to_string(),
            false,
        );
        cache.set(
            "10:20:mr:2:detail".to_string(),
            "project-20".to_string(),
            false,
        );

        cache.invalidate_project(2);

        assert_eq!(cache.get("10:2:kanban:abc"), None);
        assert_eq!(
            cache.get("10:12:issue:2:mrs").as_deref(),
            Some("project-12")
        );
        assert_eq!(
            cache.get("10:20:mr:2:detail").as_deref(),
            Some("project-20")
        );
    }
}

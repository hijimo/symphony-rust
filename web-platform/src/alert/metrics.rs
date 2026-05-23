use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;

use crate::concurrency::ConcurrencyManager;
use crate::models::ServiceStatus;
use crate::process_manager::ProcessManager;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A snapshot of all metrics collected at a point in time.
#[derive(Debug, Clone)]
pub struct MetricSnapshot {
    /// Running tasks per project: project_id -> list of running tasks.
    pub running_tasks: HashMap<i64, Vec<RunningTask>>,
    /// Service health per project.
    pub service_health: HashMap<i64, ServiceHealthStatus>,
    /// Global concurrency metrics.
    pub concurrency: ConcurrencyMetrics,
    /// API health: platform name -> consecutive failure count.
    pub api_health: HashMap<String, u64>,
    /// When this snapshot was collected.
    pub collected_at: DateTime<Utc>,
}

/// A currently running task (agent working on an issue).
#[derive(Debug, Clone)]
pub struct RunningTask {
    pub agent_id: String,
    pub issue_iid: i64,
    pub issue_title: String,
    pub project_id: i64,
    pub started_at: DateTime<Utc>,
    pub elapsed_seconds: i64,
}

/// Health status of a project's Symphony service.
#[derive(Debug, Clone)]
pub enum ServiceHealthStatus {
    Running,
    Stopped,
    Crashed {
        exit_code: i32,
        crashed_at: DateTime<Utc>,
    },
}

/// Global concurrency metrics.
#[derive(Debug, Clone)]
pub struct ConcurrencyMetrics {
    pub global_max: i64,
    pub global_active: i64,
    pub saturated_since: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// MetricCollector trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait MetricCollector: Send + Sync {
    /// Collect a full metrics snapshot from all sources.
    async fn collect(&self) -> MetricSnapshot;

    /// Record a task failure event (increments consecutive failure counter).
    fn record_task_failure(&self, project_id: i64, agent_id: &str, issue_iid: i64);

    /// Record a task success event (resets consecutive failure counter).
    fn record_task_success(&self, project_id: i64);

    /// Record an API call failure for a platform.
    fn record_api_failure(&self, platform: &str);

    /// Record an API call success for a platform (resets failure counter).
    fn record_api_success(&self, platform: &str);

    /// Get the consecutive failure count for a project.
    fn get_consecutive_failures(&self, project_id: i64) -> u64;

    /// Get the consecutive API failure count for a platform.
    fn get_api_consecutive_failures(&self, platform: &str) -> u64;

    /// Record a service crash event.
    fn record_service_crash(&self, project_id: i64, exit_code: i32);
}

// ---------------------------------------------------------------------------
// DefaultMetricCollector implementation
// ---------------------------------------------------------------------------

/// Tracks per-project crash events for the current evaluation cycle.
#[derive(Debug)]
struct CrashEvent {
    exit_code: i32,
    crashed_at: DateTime<Utc>,
}

/// Default implementation that reads from ProcessManager and ConcurrencyManager.
pub struct DefaultMetricCollector {
    pub(crate) process_manager: ProcessManager,
    concurrency_manager: Arc<ConcurrencyManager>,

    /// project_id -> consecutive task failure count
    task_failures: DashMap<i64, AtomicU64>,
    /// platform -> consecutive API failure count
    api_failures: DashMap<String, AtomicU64>,
    /// project_id -> most recent crash event (cleared after collection)
    crash_events: DashMap<i64, CrashEvent>,
    /// Tracks when concurrency first became saturated (None = not saturated).
    saturated_since: std::sync::Mutex<Option<DateTime<Utc>>>,
}

impl DefaultMetricCollector {
    pub fn new(
        process_manager: ProcessManager,
        concurrency_manager: Arc<ConcurrencyManager>,
    ) -> Self {
        Self {
            process_manager,
            concurrency_manager,
            task_failures: DashMap::new(),
            api_failures: DashMap::new(),
            crash_events: DashMap::new(),
            saturated_since: std::sync::Mutex::new(None),
        }
    }

    /// Update saturation tracking based on current concurrency state.
    fn update_saturation(&self, active: i64, max: i64) -> Option<DateTime<Utc>> {
        let mut guard = self.saturated_since.lock().unwrap();
        if active >= max && max > 0 {
            // Currently saturated
            if guard.is_none() {
                *guard = Some(Utc::now());
            }
            *guard
        } else {
            // Not saturated — reset
            *guard = None;
            None
        }
    }
}

#[async_trait]
impl MetricCollector for DefaultMetricCollector {
    async fn collect(&self) -> MetricSnapshot {
        let now = Utc::now();

        // --- Service health from ProcessManager ---
        let mut service_health: HashMap<i64, ServiceHealthStatus> = HashMap::new();
        for entry in self.process_manager.processes.iter() {
            let project_id = *entry.key();
            let state = entry.value();

            // Check if there's a pending crash event for this project
            if let Some((_, crash)) = self.crash_events.remove(&project_id) {
                service_health.insert(
                    project_id,
                    ServiceHealthStatus::Crashed {
                        exit_code: crash.exit_code,
                        crashed_at: crash.crashed_at,
                    },
                );
            } else {
                let status = match state.status {
                    ServiceStatus::Running | ServiceStatus::Starting => {
                        ServiceHealthStatus::Running
                    }
                    _ => ServiceHealthStatus::Stopped,
                };
                service_health.insert(project_id, status);
            }
        }

        // Also include crash events for projects not currently in ProcessManager
        // (they may have been removed after crashing)
        let remaining_crashes: Vec<(i64, CrashEvent)> = self
            .crash_events
            .iter()
            .map(|e| {
                (
                    *e.key(),
                    CrashEvent {
                        exit_code: e.value().exit_code,
                        crashed_at: e.value().crashed_at,
                    },
                )
            })
            .collect();
        for (project_id, crash) in remaining_crashes {
            self.crash_events.remove(&project_id);
            service_health
                .entry(project_id)
                .or_insert(ServiceHealthStatus::Crashed {
                    exit_code: crash.exit_code,
                    crashed_at: crash.crashed_at,
                });
        }

        // --- Concurrency metrics ---
        let global_active = self
            .concurrency_manager
            .global_active
            .load(std::sync::atomic::Ordering::Relaxed);
        let global_max = self
            .concurrency_manager
            .global_max
            .load(std::sync::atomic::Ordering::Relaxed);
        let saturated_since = self.update_saturation(global_active, global_max);

        let concurrency = ConcurrencyMetrics {
            global_max,
            global_active,
            saturated_since,
        };

        // --- API health ---
        let mut api_health: HashMap<String, u64> = HashMap::new();
        for entry in self.api_failures.iter() {
            api_health.insert(entry.key().clone(), entry.value().load(Ordering::Relaxed));
        }

        // --- Running tasks (placeholder — actual task tracking requires integration) ---
        // In the current architecture, running tasks are tracked externally.
        // The alert engine evaluates tasks based on ProcessManager state.
        // For now, we provide an empty map; task_timeout evaluation will be
        // driven by external event injection in a future integration step.
        let running_tasks: HashMap<i64, Vec<RunningTask>> = HashMap::new();

        MetricSnapshot {
            running_tasks,
            service_health,
            concurrency,
            api_health,
            collected_at: now,
        }
    }

    fn record_task_failure(&self, project_id: i64, _agent_id: &str, _issue_iid: i64) {
        self.task_failures
            .entry(project_id)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    fn record_task_success(&self, project_id: i64) {
        if let Some(counter) = self.task_failures.get(&project_id) {
            counter.store(0, Ordering::Relaxed);
        }
    }

    fn record_api_failure(&self, platform: &str) {
        self.api_failures
            .entry(platform.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    fn record_api_success(&self, platform: &str) {
        if let Some(counter) = self.api_failures.get(platform) {
            counter.store(0, Ordering::Relaxed);
        }
    }

    fn get_consecutive_failures(&self, project_id: i64) -> u64 {
        self.task_failures
            .get(&project_id)
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    fn get_api_consecutive_failures(&self, platform: &str) -> u64 {
        self.api_failures
            .get(platform)
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    fn record_service_crash(&self, project_id: i64, exit_code: i32) {
        self.crash_events.insert(
            project_id,
            CrashEvent {
                exit_code,
                crashed_at: Utc::now(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concurrency::ConcurrencyManager;
    use crate::process_manager::ProcessManager;
    use std::sync::Arc;

    /// Helper: create a DefaultMetricCollector for testing.
    fn make_collector() -> DefaultMetricCollector {
        let pm = ProcessManager::new();
        let cm = Arc::new(ConcurrencyManager::new(10));
        DefaultMetricCollector::new(pm, cm)
    }

    #[tokio::test]
    async fn test_metric_snapshot_empty_state() {
        let collector = make_collector();
        let snapshot = collector.collect().await;

        assert!(snapshot.running_tasks.is_empty());
        assert!(snapshot.service_health.is_empty());
        assert!(snapshot.api_health.is_empty());
        assert_eq!(snapshot.concurrency.global_max, 10);
        assert_eq!(snapshot.concurrency.global_active, 0);
        assert!(snapshot.concurrency.saturated_since.is_none());
    }

    #[test]
    fn test_record_api_failure_increments() {
        let collector = make_collector();

        assert_eq!(collector.get_api_consecutive_failures("gitlab"), 0);

        collector.record_api_failure("gitlab");
        assert_eq!(collector.get_api_consecutive_failures("gitlab"), 1);

        collector.record_api_failure("gitlab");
        assert_eq!(collector.get_api_consecutive_failures("gitlab"), 2);

        collector.record_api_failure("gitlab");
        assert_eq!(collector.get_api_consecutive_failures("gitlab"), 3);

        // Different platform should be independent
        assert_eq!(collector.get_api_consecutive_failures("github"), 0);
        collector.record_api_failure("github");
        assert_eq!(collector.get_api_consecutive_failures("github"), 1);
        assert_eq!(collector.get_api_consecutive_failures("gitlab"), 3);

        // Success resets the counter
        collector.record_api_success("gitlab");
        assert_eq!(collector.get_api_consecutive_failures("gitlab"), 0);
        // github should remain unchanged
        assert_eq!(collector.get_api_consecutive_failures("github"), 1);
    }

    #[test]
    fn test_record_service_crash() {
        let collector = make_collector();

        // Record a crash
        collector.record_service_crash(1, 137);

        // Verify crash event is stored
        assert!(collector.crash_events.contains_key(&1));
        let event = collector.crash_events.get(&1).unwrap();
        assert_eq!(event.exit_code, 137);
    }

    #[test]
    fn test_reset_api_failures() {
        let collector = make_collector();

        // Record some failures
        collector.record_api_failure("gitlab");
        collector.record_api_failure("gitlab");
        collector.record_api_failure("gitlab");
        assert_eq!(collector.get_api_consecutive_failures("gitlab"), 3);

        // Success resets to 0
        collector.record_api_success("gitlab");
        assert_eq!(collector.get_api_consecutive_failures("gitlab"), 0);

        // Resetting a platform that was never recorded is a no-op
        collector.record_api_success("unknown_platform");
        assert_eq!(
            collector.get_api_consecutive_failures("unknown_platform"),
            0
        );
    }
}

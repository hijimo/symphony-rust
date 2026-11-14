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

/// `Human Review` and document review states (`doc:*-review`) wait for people.
fn waits_for_human(state: &str) -> bool {
    let s = state.trim().to_lowercase().replace([' ', '-'], "_");
    s == "human_review" || (s.starts_with("doc:") && s.ends_with("review"))
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
    /// Polled rust-platform state (P2 design §3 F1); source of running tasks.
    runtime_state: Option<Arc<crate::services::runtime_state::RuntimeStateCache>>,
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
            runtime_state: None,
        }
    }

    /// Read running tasks from the runtime state cache.
    pub fn with_runtime_state(
        mut self,
        cache: Arc<crate::services::runtime_state::RuntimeStateCache>,
    ) -> Self {
        self.runtime_state = Some(cache);
        self
    }

    /// Running agent sessions per project. States that wait for a human are
    /// skipped even if a WORKFLOW lists them as active, so they never raise
    /// task-timeout alerts (P2 design §5 A5).
    fn running_tasks(&self, now: DateTime<Utc>) -> HashMap<i64, Vec<RunningTask>> {
        let mut out: HashMap<i64, Vec<RunningTask>> = HashMap::new();
        let Some(cache) = &self.runtime_state else {
            return out;
        };
        for ((project_id, instance), snap) in cache.snapshots() {
            for row in snap.running {
                if waits_for_human(&row.state) {
                    continue;
                }
                let Some(iid) = row
                    .issue_identifier
                    .trim_start_matches('#')
                    .parse::<i64>()
                    .ok()
                    .or_else(|| row.issue_id.parse().ok())
                else {
                    continue;
                };
                let Ok(started) = DateTime::parse_from_rfc3339(&row.started_at) else {
                    continue;
                };
                let started_at = started.with_timezone(&Utc);
                let inst = match instance {
                    crate::services::runtime_state::Instance::Dev => "dev",
                    crate::services::runtime_state::Instance::Test => "test",
                };
                out.entry(project_id).or_default().push(RunningTask {
                    agent_id: format!("{inst}:{}", row.issue_identifier),
                    issue_iid: iid,
                    issue_title: row.issue_identifier.clone(),
                    project_id,
                    started_at,
                    elapsed_seconds: (now - started_at).num_seconds().max(0),
                });
            }
        }
        out
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
        let global_active = self.process_manager.active_process_count();
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

        // --- Running tasks from the polled rust-platform state ---
        let running_tasks = self.running_tasks(now);

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
    async fn test_running_tasks_come_from_runtime_state_and_skip_waiting_states() {
        use crate::services::runtime_state::RuntimeStateCache;
        use axum::{routing::get, Json, Router};

        let started = (Utc::now() - chrono::Duration::minutes(45)).to_rfc3339();
        let body = serde_json::json!({
            "running": [
                {"issue_id": "42", "issue_identifier": "#42", "state": "In Progress",
                 "turn_count": 2, "started_at": started, "last_event_at": null,
                 "tokens": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}},
                {"issue_id": "43", "issue_identifier": "#43", "state": "Human Review",
                 "turn_count": 1, "started_at": started, "last_event_at": null},
                {"issue_id": "44", "issue_identifier": "#44", "state": "doc:plan-review",
                 "turn_count": 1, "started_at": started, "last_event_at": null},
                {"issue_id": "x", "issue_identifier": "ENG-1", "state": "Todo",
                 "turn_count": 1, "started_at": "not a date", "last_event_at": null}
            ],
            "retrying": []
        });
        let app = Router::new().route("/api/v1/state", get(move || async move { Json(body) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let pm = ProcessManager::new();
        pm.set_state(
            9,
            crate::process_manager::ProcessState {
                pid: 1,
                started_at: Utc::now(),
                status: ServiceStatus::Running,
                restart_count: 0,
                api_port: Some(port),
            },
        );
        let cm = Arc::new(ConcurrencyManager::new(10));
        let runtime = Arc::new(RuntimeStateCache::new());
        runtime.poll_once(&pm, &cm, |_| "p".into()).await;

        let collector = DefaultMetricCollector::new(pm, cm).with_runtime_state(runtime);
        let snap = collector.collect().await;
        let tasks = snap.running_tasks.get(&9).expect("project 9");
        assert_eq!(tasks.len(), 1, "waiting-human and unparsable rows skipped");
        let t = &tasks[0];
        assert_eq!(t.issue_iid, 42);
        assert_eq!(t.project_id, 9);
        assert_eq!(t.agent_id, "dev:#42");
        assert!(t.elapsed_seconds >= 45 * 60 - 5, "{}", t.elapsed_seconds);
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

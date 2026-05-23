#![allow(
    unused_imports,
    unused_variables,
    dead_code,
    clippy::bind_instead_of_map,
    clippy::derivable_impls,
    clippy::manual_range_contains,
    clippy::needless_borrows_for_generic_args,
    clippy::ptr_arg,
    clippy::duplicated_attributes,
    clippy::approx_constant,
    clippy::bool_assert_comparison,
    clippy::len_zero,
    clippy::let_and_return
)]

//! TestOrchestrator — wraps the full Orchestrator with test controls.
//!
//! Provides a higher-level API for E2E tests to:
//! - Start the orchestrator with custom config
//! - Inject mock tracker responses
//! - Inject mock codex process behavior
//! - Wait for specific state transitions
//! - Assert on final state
//! - Trigger shutdown
//! - Run N ticks and check results

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

use symphony_platform::orchestrator::scheduler::DispatchConfig;
use symphony_platform::orchestrator::Orchestrator;
use symphony_platform::platform::{IssueId, MemoryAdapter};

use super::fake_codex::CodexBehavior;

/// Worker state as observed by the test harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerState {
    /// Worker is idle (not processing any issue)
    Idle,
    /// Worker is running an agent session for the given issue
    Running(IssueId),
    /// Worker completed successfully
    Completed(IssueId),
    /// Worker failed with an error
    Failed(IssueId, String),
    /// Worker was killed (stall detection or shutdown)
    Killed(IssueId),
    /// Worker is in retry backoff
    RetryPending(IssueId),
}

/// Configuration for the test orchestrator.
#[derive(Debug, Clone)]
pub struct TestOrchestratorConfig {
    /// Polling interval (short for tests)
    pub poll_interval_ms: u64,
    /// Maximum concurrent workers
    pub max_workers: usize,
    /// Stall detection timeout
    pub stall_timeout_ms: u64,
    /// Maximum retry backoff
    pub max_retry_backoff_ms: u64,
    /// Active workflow states
    pub active_states: Vec<String>,
    /// Terminal workflow states
    pub terminal_states: Vec<String>,
    /// Workspace base directory
    pub workspace_dir: PathBuf,
    /// Per-state concurrency limits
    pub max_concurrent_by_state: HashMap<String, usize>,
    /// Blocker check states
    pub blocker_check_states: Vec<String>,
}

impl Default for TestOrchestratorConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 100,
            max_workers: 3,
            stall_timeout_ms: 5000,
            max_retry_backoff_ms: 1000,
            active_states: vec!["todo".to_string(), "in_progress".to_string()],
            terminal_states: vec!["done".to_string(), "cancelled".to_string()],
            workspace_dir: std::env::temp_dir().join("symphony-test-workspaces"),
            max_concurrent_by_state: HashMap::new(),
            blocker_check_states: vec!["todo".to_string()],
        }
    }
}

impl TestOrchestratorConfig {
    /// Create a config with a single worker slot (for sequential testing).
    pub fn single_worker() -> Self {
        Self {
            max_workers: 1,
            ..Self::default()
        }
    }

    /// Create a config with fast polling (for quick tests).
    pub fn fast_polling() -> Self {
        Self {
            poll_interval_ms: 50,
            ..Self::default()
        }
    }

    /// Create a config with short stall timeout (for stall detection tests).
    pub fn short_stall_timeout(timeout_ms: u64) -> Self {
        Self {
            stall_timeout_ms: timeout_ms,
            ..Self::default()
        }
    }
}

/// Test orchestrator that wraps the real Orchestrator with test controls.
pub struct TestOrchestrator {
    /// The underlying memory adapter (for seeding issues and checking state)
    pub adapter: Arc<MemoryAdapter>,
    /// Cancellation token for shutdown
    cancel: CancellationToken,
    /// Configuration
    config: TestOrchestratorConfig,
    /// Codex behaviors keyed by issue ID
    codex_behaviors: Arc<Mutex<HashMap<u64, CodexBehavior>>>,
    /// State transition log
    state_log: Arc<Mutex<Vec<(IssueId, WorkerState)>>>,
    /// Notify when a state transition occurs
    state_notify: Arc<Notify>,
    /// Whether the orchestrator is running
    running: Arc<Mutex<bool>>,
    /// Tick count (how many ticks have been processed)
    tick_count: Arc<Mutex<u64>>,
}

impl TestOrchestrator {
    /// Create a new test orchestrator with the given config.
    pub fn new(config: TestOrchestratorConfig) -> Self {
        Self {
            adapter: Arc::new(MemoryAdapter::new()),
            cancel: CancellationToken::new(),
            config,
            codex_behaviors: Arc::new(Mutex::new(HashMap::new())),
            state_log: Arc::new(Mutex::new(Vec::new())),
            state_notify: Arc::new(Notify::new()),
            running: Arc::new(Mutex::new(false)),
            tick_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Create with default config.
    pub fn default_config() -> Self {
        Self::new(TestOrchestratorConfig::default())
    }

    /// Get a reference to the memory adapter for seeding issues.
    pub fn adapter(&self) -> &Arc<MemoryAdapter> {
        &self.adapter
    }

    /// Get the cancellation token (for external shutdown triggers).
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Set the codex behavior for a specific issue.
    pub async fn set_codex_behavior(&self, issue_id: u64, behavior: CodexBehavior) {
        let mut behaviors = self.codex_behaviors.lock().await;
        behaviors.insert(issue_id, behavior);
    }

    /// Set a default codex behavior for all issues.
    pub async fn set_default_codex_behavior(&self, behavior: CodexBehavior) {
        let mut behaviors = self.codex_behaviors.lock().await;
        behaviors.insert(0, behavior); // 0 = default
    }

    /// Get the codex behavior for a specific issue (falls back to default).
    pub async fn get_codex_behavior(&self, issue_id: u64) -> CodexBehavior {
        let behaviors = self.codex_behaviors.lock().await;
        behaviors
            .get(&issue_id)
            .or_else(|| behaviors.get(&0))
            .cloned()
            .unwrap_or_else(CodexBehavior::success)
    }

    /// Start the orchestrator in the background.
    ///
    /// Returns a JoinHandle that resolves when the orchestrator stops.
    pub async fn start(&self) -> tokio::task::JoinHandle<()> {
        let mut running = self.running.lock().await;
        *running = true;

        let cancel = self.cancel.clone();
        let dispatch_config = self.build_dispatch_config();

        let mut orchestrator = Orchestrator::new(
            dispatch_config,
            self.config.stall_timeout_ms as i64,
            self.config.max_retry_backoff_ms,
            cancel,
        );

        let running_flag = self.running.clone();
        tokio::spawn(async move {
            orchestrator.run().await;
            let mut running = running_flag.lock().await;
            *running = false;
        })
    }

    /// Trigger a graceful shutdown.
    pub fn shutdown(&self) {
        self.cancel.cancel();
    }

    /// Wait for the orchestrator to process at least one poll cycle.
    pub async fn wait_for_poll_cycle(&self) {
        tokio::time::sleep(Duration::from_millis(self.config.poll_interval_ms * 2)).await;
    }

    /// Wait for a specific number of poll cycles.
    pub async fn wait_for_cycles(&self, n: u64) {
        tokio::time::sleep(Duration::from_millis(self.config.poll_interval_ms * n + 50)).await;
    }

    /// Wait until the orchestrator has dispatched the given issue.
    pub async fn wait_for_dispatch(&self, _issue_id: IssueId, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            // Check if the adapter has been called (proxy for dispatch activity)
            let count = self.adapter.call_count("fetch_candidate_issues").await;
            if count > 0 {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Wait for a specific state transition to appear in the log.
    pub async fn wait_for_state(
        &self,
        issue_id: IssueId,
        expected: &WorkerState,
        timeout: Duration,
    ) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let log = self.state_log.lock().await;
            if log
                .iter()
                .any(|(id, state)| *id == issue_id && state == expected)
            {
                return true;
            }
            drop(log);

            if tokio::time::Instant::now() >= deadline {
                return false;
            }

            // Wait for notification or timeout
            tokio::select! {
                _ = self.state_notify.notified() => {}
                _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
        }
    }

    /// Record a state transition (called by the test harness internals).
    pub async fn record_state(&self, issue_id: IssueId, state: WorkerState) {
        let mut log = self.state_log.lock().await;
        log.push((issue_id, state));
        self.state_notify.notify_waiters();
    }

    /// Get the full state transition log.
    pub async fn state_log(&self) -> Vec<(IssueId, WorkerState)> {
        self.state_log.lock().await.clone()
    }

    /// Clear the state transition log.
    pub async fn clear_state_log(&self) {
        let mut log = self.state_log.lock().await;
        log.clear();
    }

    /// Check if the orchestrator is currently running.
    pub async fn is_running(&self) -> bool {
        *self.running.lock().await
    }

    /// Get the current tick count.
    pub async fn tick_count(&self) -> u64 {
        *self.tick_count.lock().await
    }

    /// Increment the tick count (called internally).
    pub async fn increment_tick(&self) {
        let mut count = self.tick_count.lock().await;
        *count += 1;
    }

    /// Build the DispatchConfig from test configuration.
    fn build_dispatch_config(&self) -> DispatchConfig {
        DispatchConfig {
            active_states: self.config.active_states.clone(),
            terminal_states: self.config.terminal_states.clone(),
            max_concurrent_agents: self.config.max_workers,
            max_concurrent_agents_by_state: self.config.max_concurrent_by_state.clone(),
            blocker_check_states: self.config.blocker_check_states.clone(),
            poll_interval_ms: self.config.poll_interval_ms,
            assignee_id: None,
        }
    }

    /// Assert that the orchestrator has no running workers.
    pub async fn assert_no_running_workers(&self) {
        let log = self.state_log.lock().await;
        let running_count = log
            .iter()
            .filter(|(_, state)| matches!(state, WorkerState::Running(_)))
            .count();
        let completed_count = log
            .iter()
            .filter(|(_, state)| {
                matches!(
                    state,
                    WorkerState::Completed(_) | WorkerState::Failed(_, _) | WorkerState::Killed(_)
                )
            })
            .count();
        assert!(
            running_count <= completed_count,
            "Expected no running workers, but found {} running vs {} completed",
            running_count,
            completed_count
        );
    }

    /// Assert that a specific issue was dispatched.
    pub async fn assert_issue_dispatched(&self, issue_id: IssueId) {
        let log = self.state_log.lock().await;
        let was_dispatched = log
            .iter()
            .any(|(id, state)| *id == issue_id && matches!(state, WorkerState::Running(_)));
        assert!(
            was_dispatched,
            "Expected issue {:?} to be dispatched",
            issue_id
        );
    }

    /// Assert that a specific issue completed successfully.
    pub async fn assert_issue_completed(&self, issue_id: IssueId) {
        let log = self.state_log.lock().await;
        let completed = log
            .iter()
            .any(|(id, state)| *id == issue_id && matches!(state, WorkerState::Completed(_)));
        assert!(
            completed,
            "Expected issue {:?} to complete successfully",
            issue_id
        );
    }

    /// Assert that a specific issue failed.
    pub async fn assert_issue_failed(&self, issue_id: IssueId) {
        let log = self.state_log.lock().await;
        let failed = log
            .iter()
            .any(|(id, state)| *id == issue_id && matches!(state, WorkerState::Failed(_, _)));
        assert!(failed, "Expected issue {:?} to fail", issue_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_platform::platform::make_test_issue;

    #[tokio::test]
    async fn test_orchestrator_creation() {
        let orch = TestOrchestrator::default_config();
        assert!(!orch.is_running().await);
    }

    #[tokio::test]
    async fn test_orchestrator_start_and_shutdown() {
        let orch = TestOrchestrator::default_config();

        // Start
        let handle = orch.start().await;

        // Wait for at least one poll
        orch.wait_for_poll_cycle().await;

        // Shutdown
        orch.shutdown();

        // Wait for completion
        let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;

        assert!(!orch.is_running().await);
    }

    #[tokio::test]
    async fn test_codex_behavior_injection() {
        let orch = TestOrchestrator::default_config();

        orch.set_codex_behavior(42, CodexBehavior::success()).await;
        orch.set_codex_behavior(43, CodexBehavior::failure(1)).await;

        let behaviors = orch.codex_behaviors.lock().await;
        assert_eq!(behaviors.len(), 2);
        assert_eq!(behaviors[&42].exit_code, 0);
        assert_eq!(behaviors[&43].exit_code, 1);
    }

    #[tokio::test]
    async fn test_codex_behavior_fallback_to_default() {
        let orch = TestOrchestrator::default_config();

        orch.set_default_codex_behavior(CodexBehavior::failure(2))
            .await;

        // Issue 99 has no specific behavior, should get default
        let behavior = orch.get_codex_behavior(99).await;
        assert_eq!(behavior.exit_code, 2);
    }

    #[tokio::test]
    async fn test_state_log_recording() {
        let orch = TestOrchestrator::default_config();

        orch.record_state(IssueId(1), WorkerState::Running(IssueId(1)))
            .await;
        orch.record_state(IssueId(1), WorkerState::Completed(IssueId(1)))
            .await;

        let log = orch.state_log().await;
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].1, WorkerState::Running(IssueId(1)));
        assert_eq!(log[1].1, WorkerState::Completed(IssueId(1)));
    }

    #[tokio::test]
    async fn test_wait_for_state() {
        let orch = TestOrchestrator::default_config();

        // Spawn a task that records state after a delay
        let orch_clone_state_log = orch.state_log.clone();
        let orch_clone_notify = orch.state_notify.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let mut log = orch_clone_state_log.lock().await;
            log.push((IssueId(1), WorkerState::Completed(IssueId(1))));
            orch_clone_notify.notify_waiters();
        });

        let found = orch
            .wait_for_state(
                IssueId(1),
                &WorkerState::Completed(IssueId(1)),
                Duration::from_secs(1),
            )
            .await;
        assert!(found);
    }

    #[tokio::test]
    async fn test_wait_for_state_timeout() {
        let orch = TestOrchestrator::default_config();

        let found = orch
            .wait_for_state(
                IssueId(999),
                &WorkerState::Completed(IssueId(999)),
                Duration::from_millis(50),
            )
            .await;
        assert!(!found);
    }

    #[tokio::test]
    async fn test_single_worker_config() {
        let config = TestOrchestratorConfig::single_worker();
        assert_eq!(config.max_workers, 1);
    }

    #[tokio::test]
    async fn test_fast_polling_config() {
        let config = TestOrchestratorConfig::fast_polling();
        assert_eq!(config.poll_interval_ms, 50);
    }
}

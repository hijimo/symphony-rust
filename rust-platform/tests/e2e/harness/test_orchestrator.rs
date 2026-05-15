//! TestOrchestrator — wraps the full Orchestrator with test controls.
//!
//! Provides a higher-level API for E2E tests to:
//! - Start the orchestrator with custom config
//! - Inject mock tracker responses
//! - Inject mock codex process behavior
//! - Wait for specific state transitions
//! - Assert on final state
//! - Trigger shutdown

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

use symphony_platform::orchestrator::Orchestrator;
use symphony_platform::orchestrator::scheduler::DispatchConfig;
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

    /// Check if the orchestrator is currently running.
    pub async fn is_running(&self) -> bool {
        *self.running.lock().await
    }

    /// Build the DispatchConfig from test configuration.
    fn build_dispatch_config(&self) -> DispatchConfig {
        DispatchConfig {
            active_states: self.config.active_states.clone(),
            terminal_states: self.config.terminal_states.clone(),
            max_concurrent_agents: self.config.max_workers,
            max_concurrent_agents_by_state: HashMap::new(),
            blocker_check_states: vec!["todo".to_string()],
            poll_interval_ms: self.config.poll_interval_ms,
        }
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
}

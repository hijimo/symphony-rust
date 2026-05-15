//! Orchestrator — event-driven state machine for issue dispatch and lifecycle.
//!
//! This is the complete rewrite implementing SPEC sections 7-8:
//! - Event-driven architecture with tokio::sync::mpsc channel
//! - Poll-and-dispatch tick logic
//! - Candidate eligibility and sorting
//! - Concurrency control (global + per-state)
//! - Retry queue with exponential backoff
//! - Stall detection and reconciliation
//! - Graceful shutdown

pub mod scheduler;
pub mod retry;
pub mod reconciler;

use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::models::{
    CodexEventUpdate, Issue, LiveSession, OrchestratorEvent, OrchestratorState,
    RetryKind, RunningEntry, ShutdownConfig,
};
use self::reconciler::reconcile_stalled_runs;
use self::retry::{compute_retry_delay, release_claim, schedule_retry};
use self::scheduler::{
    schedule_immediate_tick, schedule_next_tick, should_dispatch, sort_for_dispatch,
    DispatchConfig,
};

/// The Orchestrator drives the event-driven state machine.
///
/// It owns the single authoritative runtime state and processes all events
/// through a serialized mpsc channel, ensuring no concurrent state mutations.
pub struct Orchestrator {
    pub state: OrchestratorState,
    pub dispatch_config: DispatchConfig,
    pub stall_timeout_ms: i64,
    pub max_retry_backoff_ms: u64,
    event_rx: mpsc::Receiver<OrchestratorEvent>,
    event_tx: mpsc::Sender<OrchestratorEvent>,
    cancel: CancellationToken,
    shutdown_config: ShutdownConfig,
    /// Handle to the current tick timer (so we can abort on shutdown).
    tick_timer: Option<tokio::task::JoinHandle<()>>,
}

impl Orchestrator {
    /// Create a new event-driven Orchestrator.
    pub fn new(
        dispatch_config: DispatchConfig,
        stall_timeout_ms: i64,
        max_retry_backoff_ms: u64,
        cancel: CancellationToken,
    ) -> Self {
        let (event_tx, event_rx) = mpsc::channel(256);
        let poll_interval_ms = dispatch_config.poll_interval_ms;
        let max_concurrent = dispatch_config.max_concurrent_agents;

        Self {
            state: OrchestratorState::new(poll_interval_ms, max_concurrent),
            dispatch_config,
            stall_timeout_ms,
            max_retry_backoff_ms,
            event_rx,
            event_tx,
            cancel,
            shutdown_config: ShutdownConfig::default(),
            tick_timer: None,
        }
    }

    /// Get a clone of the event sender (for external components to send events).
    pub fn event_sender(&self) -> mpsc::Sender<OrchestratorEvent> {
        self.event_tx.clone()
    }

    /// Run the orchestrator event loop until shutdown.
    pub async fn run(&mut self) {
        tracing::info!("Orchestrator starting event loop");

        // Schedule immediate first tick
        self.tick_timer = Some(schedule_immediate_tick(&self.event_tx));

        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => {
                    tracing::info!("Orchestrator received cancellation signal");
                    self.handle_shutdown().await;
                    break;
                }
                event = self.event_rx.recv() => {
                    match event {
                        Some(OrchestratorEvent::Shutdown) => {
                            tracing::info!("Orchestrator received Shutdown event");
                            self.handle_shutdown().await;
                            break;
                        }
                        Some(evt) => self.handle_event(evt).await,
                        None => {
                            tracing::warn!("Event channel closed, shutting down");
                            break;
                        }
                    }
                }
            }
        }

        tracing::info!("Orchestrator event loop exited");
    }

    /// Dispatch a single event.
    async fn handle_event(&mut self, event: OrchestratorEvent) {
        match event {
            OrchestratorEvent::Tick => self.on_tick().await,
            OrchestratorEvent::WorkerExitNormal { issue_id } => {
                self.on_worker_exit_normal(&issue_id).await;
            }
            OrchestratorEvent::WorkerExitAbnormal { issue_id, error } => {
                self.on_worker_exit_abnormal(&issue_id, &error).await;
            }
            OrchestratorEvent::CodexUpdate { issue_id, update } => {
                self.on_codex_update(&issue_id, update);
            }
            OrchestratorEvent::RetryFired { issue_id } => {
                self.on_retry_fired(&issue_id).await;
            }
            OrchestratorEvent::ConfigReloaded => {
                self.on_config_reloaded();
            }
            OrchestratorEvent::ForceRefresh => {
                self.on_tick().await;
            }
            OrchestratorEvent::Shutdown => {
                // Handled in the main loop
            }
        }
    }

    /// Poll-and-dispatch tick (SPEC section 8.1-8.2).
    async fn on_tick(&mut self) {
        // 0. Skip if shutting down
        if self.state.shutting_down {
            return;
        }

        // 1. GC completed records
        self.state.gc_completed();

        // 2. Defensive config check (every N ticks)
        self.state.tick_count += 1;

        // 3. Reconcile running issues (stall detection)
        let force_killed = reconcile_stalled_runs(&mut self.state, self.stall_timeout_ms);
        for entry in force_killed {
            let delay = compute_retry_delay(
                entry.attempt + 1,
                &RetryKind::Failure,
                self.max_retry_backoff_ms,
            );
            schedule_retry(
                &mut self.state,
                &entry.issue_id,
                &entry.identifier,
                entry.attempt + 1,
                RetryKind::Failure,
                delay,
                Some("stall hard deadline exceeded".to_string()),
                &self.event_tx,
            );
        }

        // 4-9: Dispatch logic would fetch candidates from tracker here.
        // The actual tracker integration is handled by the caller providing
        // candidates via dispatch_candidates().

        // Schedule next tick
        self.tick_timer = Some(schedule_next_tick(
            &self.event_tx,
            self.state.poll_interval_ms,
        ));
    }

    /// Process fetched candidates and dispatch eligible ones.
    /// Called by the integration layer after fetching from the tracker.
    pub fn dispatch_candidates(&mut self, mut candidates: Vec<Issue>) {
        if self.state.shutting_down {
            return;
        }

        // Sort for dispatch priority
        sort_for_dispatch(&mut candidates);

        // Dispatch eligible issues while slots remain
        for issue in candidates {
            if !self.state.has_global_slots() {
                break;
            }
            if should_dispatch(&issue, &self.state, &self.dispatch_config) {
                self.claim_issue(&issue);
            }
        }
    }

    /// Claim an issue for dispatch (adds to claimed set).
    /// The actual worker spawning is done by the integration layer.
    fn claim_issue(&mut self, issue: &Issue) {
        self.state.claimed.insert(issue.id.clone());
        tracing::info!(
            issue_id = %issue.id,
            identifier = %issue.identifier,
            "claimed issue for dispatch"
        );
    }

    /// Register a running worker for a claimed issue.
    pub fn register_running(
        &mut self,
        issue: Issue,
        worker_handle: tokio::task::JoinHandle<()>,
        cancel_token: CancellationToken,
        retry_attempt: Option<u32>,
    ) {
        let session = LiveSession::new("pending".to_string(), "0".to_string());
        let entry = RunningEntry {
            worker_handle,
            cancel_token,
            identifier: issue.identifier.clone(),
            issue: issue.clone(),
            session,
            retry_attempt,
            started_at: Instant::now(),
            started_at_utc: chrono::Utc::now(),
            cancel_sent_at: None,
        };
        self.state.running.insert(issue.id.clone(), entry);
    }

    /// Handle normal worker exit (SPEC section 7.3).
    /// Schedules a continuation retry (1s delay).
    async fn on_worker_exit_normal(&mut self, issue_id: &str) {
        if let Some(entry) = self.state.running.remove(issue_id) {
            // Update runtime totals
            self.state.codex_totals.add_runtime(entry.started_at);

            // Record completion
            self.state.completed.insert(issue_id.to_string(), Instant::now());

            tracing::info!(
                issue_id,
                identifier = %entry.identifier,
                "worker exited normally, scheduling continuation retry"
            );

            // Schedule continuation retry (fixed 1s delay)
            let delay = compute_retry_delay(1, &RetryKind::Continuation, self.max_retry_backoff_ms);
            schedule_retry(
                &mut self.state,
                issue_id,
                &entry.identifier,
                1,
                RetryKind::Continuation,
                delay,
                None,
                &self.event_tx,
            );
        }
    }

    /// Handle abnormal worker exit (SPEC section 7.3).
    /// Schedules exponential backoff retry.
    async fn on_worker_exit_abnormal(&mut self, issue_id: &str, error: &str) {
        if let Some(entry) = self.state.running.remove(issue_id) {
            // Update runtime totals
            self.state.codex_totals.add_runtime(entry.started_at);

            let attempt = entry.retry_attempt.unwrap_or(0) + 1;

            tracing::warn!(
                issue_id,
                identifier = %entry.identifier,
                error,
                attempt,
                "worker exited abnormally, scheduling failure retry"
            );

            let delay = compute_retry_delay(attempt, &RetryKind::Failure, self.max_retry_backoff_ms);
            schedule_retry(
                &mut self.state,
                issue_id,
                &entry.identifier,
                attempt,
                RetryKind::Failure,
                delay,
                Some(error.to_string()),
                &self.event_tx,
            );
        }
    }

    /// Handle Codex event update (SPEC section 7.3).
    fn on_codex_update(&mut self, issue_id: &str, update: CodexEventUpdate) {
        if let Some(entry) = self.state.running.get_mut(issue_id) {
            // Update session activity (resets stall timer)
            entry.session.touch();

            if let Some(event_type) = &update.event_type {
                entry.session.last_codex_event = Some(event_type.clone());
            }
            if let Some(msg) = &update.message {
                entry.session.last_codex_message = Some(msg.clone());
            }
            if let Some(ts) = update.timestamp {
                entry.session.last_codex_timestamp = Some(ts);
            }
            if let Some(input) = update.input_tokens {
                let delta = input.saturating_sub(entry.session.last_reported_input_tokens);
                entry.session.codex_input_tokens += delta;
                entry.session.last_reported_input_tokens = input;
                self.state.codex_totals.input_tokens += delta;
            }
            if let Some(output) = update.output_tokens {
                let delta = output.saturating_sub(entry.session.last_reported_output_tokens);
                entry.session.codex_output_tokens += delta;
                entry.session.last_reported_output_tokens = output;
                self.state.codex_totals.output_tokens += delta;
            }
            if let Some(total) = update.total_tokens {
                let delta = total.saturating_sub(entry.session.last_reported_total_tokens);
                entry.session.codex_total_tokens += delta;
                entry.session.last_reported_total_tokens = total;
                self.state.codex_totals.total_tokens += delta;
            }
            if let Some(rate_limits) = update.rate_limits {
                self.state.codex_rate_limits = Some(rate_limits);
            }
        }
    }

    /// Handle retry timer fired (SPEC section 16.6).
    async fn on_retry_fired(&mut self, issue_id: &str) {
        // 0. Skip if shutting down
        if self.state.shutting_down {
            return;
        }

        // 1. Remove retry entry (if not found, it was already released)
        let entry = match self.state.retry_attempts.remove(issue_id) {
            Some(e) => e,
            None => {
                tracing::debug!(issue_id, "retry fired but entry already removed");
                return;
            }
        };

        tracing::info!(
            issue_id,
            identifier = %entry.identifier,
            attempt = entry.attempt,
            "retry timer fired"
        );

        // The actual re-fetch and re-dispatch logic is handled by the integration
        // layer. Here we just mark that the retry has fired and the issue needs
        // re-evaluation. If no slots are available, the integration layer should
        // call reschedule_retry().
    }

    /// Reschedule a retry when no slots are available (called by integration layer).
    pub fn reschedule_no_slots(&mut self, issue_id: &str, identifier: &str, attempt: u32) {
        let delay = compute_retry_delay(attempt + 1, &RetryKind::Failure, self.max_retry_backoff_ms);
        schedule_retry(
            &mut self.state,
            issue_id,
            identifier,
            attempt + 1,
            RetryKind::Failure,
            delay,
            Some("no available orchestrator slots".to_string()),
            &self.event_tx,
        );
    }

    /// Release a claim when an issue is no longer eligible.
    pub fn release_issue_claim(&mut self, issue_id: &str) {
        release_claim(&mut self.state, issue_id);
    }

    /// Handle config reload notification.
    fn on_config_reloaded(&mut self) {
        tracing::info!("configuration reloaded, updating orchestrator state");
        // Update state from new config
        self.state.poll_interval_ms = self.dispatch_config.poll_interval_ms;
        self.state.max_concurrent_agents = self.dispatch_config.max_concurrent_agents;
    }

    /// Graceful shutdown sequence (SPEC design doc section 5.4).
    async fn handle_shutdown(&mut self) {
        tracing::info!("beginning graceful shutdown");

        // 1. Set shutting_down flag
        self.state.shutting_down = true;

        // 2. Cancel all retry timers
        for (_, entry) in self.state.retry_attempts.drain() {
            entry.timer_handle.abort();
        }

        // 3. Abort tick timer
        if let Some(timer) = self.tick_timer.take() {
            timer.abort();
        }

        // 4. Cancel all active workers
        for (_, entry) in self.state.running.iter_mut() {
            entry.cancel_token.cancel();
            entry.cancel_sent_at = Some(Instant::now());
        }

        // 5. Wait for workers to exit (with timeout)
        let drain_timeout = Duration::from_millis(self.shutdown_config.worker_drain_timeout_ms);
        let deadline = tokio::time::sleep(drain_timeout);
        tokio::pin!(deadline);

        loop {
            if self.state.running.is_empty() {
                tracing::info!("all workers exited gracefully");
                break;
            }

            tokio::select! {
                _ = &mut deadline => {
                    tracing::warn!(
                        remaining = self.state.running.len(),
                        "drain timeout reached, force-killing remaining workers"
                    );
                    for (_, entry) in self.state.running.drain() {
                        entry.worker_handle.abort();
                    }
                    break;
                }
                event = self.event_rx.recv() => {
                    match event {
                        Some(OrchestratorEvent::WorkerExitNormal { issue_id }) => {
                            self.state.running.remove(&issue_id);
                        }
                        Some(OrchestratorEvent::WorkerExitAbnormal { issue_id, .. }) => {
                            self.state.running.remove(&issue_id);
                        }
                        _ => {}
                    }
                }
            }
        }

        // 6. Clear claimed set
        self.state.claimed.clear();

        tracing::info!("graceful shutdown complete");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_orchestrator_creation() {
        let cancel = CancellationToken::new();
        let config = DispatchConfig::default();
        let orch = Orchestrator::new(config, 300_000, 300_000, cancel);

        assert_eq!(orch.state.poll_interval_ms, 30_000);
        assert_eq!(orch.state.max_concurrent_agents, 10);
        assert!(!orch.state.shutting_down);
        assert!(orch.state.running.is_empty());
        assert!(orch.state.claimed.is_empty());
    }

    #[tokio::test]
    async fn test_dispatch_candidates() {
        let cancel = CancellationToken::new();
        let config = DispatchConfig::default();
        let mut orch = Orchestrator::new(config, 300_000, 300_000, cancel);

        let issues = vec![
            Issue {
                id: "1".to_string(),
                identifier: "TEST-1".to_string(),
                title: "Test issue".to_string(),
                description: None,
                priority: Some(1),
                state: "Todo".to_string(),
                branch_name: None,
                url: None,
                labels: vec![],
                blocked_by: vec![],
                created_at: None,
                updated_at: None,
            },
        ];

        orch.dispatch_candidates(issues);
        assert!(orch.state.claimed.contains("1"));
    }

    #[tokio::test]
    async fn test_shutdown_sets_flag() {
        let cancel = CancellationToken::new();
        let config = DispatchConfig::default();
        let mut orch = Orchestrator::new(config, 300_000, 300_000, cancel);

        orch.handle_shutdown().await;
        assert!(orch.state.shutting_down);
        assert!(orch.state.claimed.is_empty());
    }

    #[tokio::test]
    async fn test_dispatch_skipped_when_shutting_down() {
        let cancel = CancellationToken::new();
        let config = DispatchConfig::default();
        let mut orch = Orchestrator::new(config, 300_000, 300_000, cancel);
        orch.state.shutting_down = true;

        let issues = vec![
            Issue {
                id: "1".to_string(),
                identifier: "TEST-1".to_string(),
                title: "Test".to_string(),
                description: None,
                priority: Some(1),
                state: "Todo".to_string(),
                branch_name: None,
                url: None,
                labels: vec![],
                blocked_by: vec![],
                created_at: None,
                updated_at: None,
            },
        ];

        orch.dispatch_candidates(issues);
        assert!(!orch.state.claimed.contains("1"));
    }
}

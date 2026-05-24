//! Orchestrator — event-driven state machine for issue dispatch and lifecycle.
//!
//! This is the complete rewrite implementing SPEC sections 7-8:
//! - Event-driven architecture with tokio::sync::mpsc channel
//! - Poll-and-dispatch tick logic with tracker integration
//! - Candidate eligibility and sorting
//! - Concurrency control (global + per-state)
//! - Retry queue with exponential backoff
//! - Stall detection and reconciliation
//! - Reconciler Part B: terminate workers for terminal issues
//! - Dispatch preflight validation
//! - Assignee routing
//! - Graceful shutdown

pub mod reconciler;
pub mod retry;
pub mod scheduler;

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use self::reconciler::{determine_reconcile_actions, reconcile_stalled_runs, ReconcileAction};
use self::retry::{compute_retry_delay, release_claim, schedule_retry, RetrySchedule};
use self::scheduler::{
    is_active_state, is_terminal_state, schedule_immediate_tick, schedule_next_tick,
    should_dispatch, sort_for_dispatch, DispatchConfig,
};
use crate::agent::runner::{AgentBlockerRef, AgentIssue, AgentRunner, IssueStateRefresher};
use crate::config::watcher::ConfigHolder;
use crate::models::{
    current_monotonic_ms, CodexEventUpdate, Issue, LiveSession, OrchestratorEvent,
    OrchestratorState, RetryKind, RunningEntry, ShutdownConfig,
};
use crate::prompt::PromptEngine;
use crate::server::api::{
    CodexTotalsJson, Counts, IssueDetailResponse, RetryRow, RunningRow, StateResponse, TokensJson,
};
use crate::tracker::{Tracker, TrackerIssue};
use crate::workspace::WorkspaceManager;

/// The Orchestrator drives the event-driven state machine.
///
/// It owns the single authoritative runtime state and processes all events
/// through a serialized mpsc channel, ensuring no concurrent state mutations.
///
/// Phase 2: Includes tracker integration, dispatch pipeline, worker spawning,
/// reconciler Part B, and retry re-evaluation.
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
    /// Tracker for fetching candidate issues and refreshing state.
    tracker: Option<Arc<dyn Tracker>>,
    /// Prompt engine for rendering issue prompts.
    prompt_engine: Option<Arc<PromptEngine>>,
    /// Workspace manager for per-issue workspace lifecycle.
    workspace_mgr: Option<Arc<WorkspaceManager>>,
    /// Config holder for snapshotting config into workers.
    config_holder: Option<Arc<ConfigHolder>>,
    /// Whether dispatch preflight has been validated this session.
    preflight_validated: bool,
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
            tracker: None,
            prompt_engine: None,
            workspace_mgr: None,
            config_holder: None,
            preflight_validated: false,
        }
    }

    /// Get a clone of the event sender (for external components to send events).
    pub fn event_sender(&self) -> mpsc::Sender<OrchestratorEvent> {
        self.event_tx.clone()
    }

    /// Set the tracker client for issue fetching and reconciliation.
    pub fn set_tracker(&mut self, tracker: Arc<dyn Tracker>) {
        self.tracker = Some(tracker);
    }

    /// Set the prompt engine for rendering issue prompts.
    pub fn set_prompt_engine(&mut self, engine: Arc<PromptEngine>) {
        self.prompt_engine = Some(engine);
    }

    /// Set the workspace manager for per-issue workspace lifecycle.
    pub fn set_workspace_mgr(&mut self, mgr: Arc<WorkspaceManager>) {
        self.workspace_mgr = Some(mgr);
    }

    /// Set the config holder for snapshotting config into workers.
    pub fn set_config_holder(&mut self, holder: Arc<ConfigHolder>) {
        self.config_holder = Some(holder);
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
                self.on_codex_update(&issue_id, *update);
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
            OrchestratorEvent::QueryState { reply } => {
                let _ = reply.send(self.state_response());
            }
            OrchestratorEvent::QueryIssue { identifier, reply } => {
                let _ = reply.send(self.issue_detail_response(&identifier));
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

        // 3. Reconcile Part A: stall detection
        let force_killed = reconcile_stalled_runs(&mut self.state, self.stall_timeout_ms);
        for entry in force_killed {
            let delay = compute_retry_delay(
                entry.attempt + 1,
                &RetryKind::Failure,
                self.max_retry_backoff_ms,
            );
            schedule_retry(
                &mut self.state,
                RetrySchedule::new(
                    &entry.issue_id,
                    &entry.identifier,
                    entry.attempt + 1,
                    RetryKind::Failure,
                    delay,
                    Some("stall hard deadline exceeded".to_string()),
                ),
                &self.event_tx,
            );
        }

        // 4. Reconciler Part B: terminate workers for terminal/invisible issues (SPEC 8.5)
        if let Some(tracker) = &self.tracker {
            let running_ids: Vec<String> = self.state.running.keys().cloned().collect();
            if !running_ids.is_empty() {
                match tracker.fetch_issue_states_by_ids(&running_ids).await {
                    Ok(fresh_issues) => {
                        let refreshed_states: Vec<(String, String)> = fresh_issues
                            .iter()
                            .map(|i| (i.id.clone(), i.state.clone()))
                            .collect();
                        let identifiers: Vec<(String, String)> = self
                            .state
                            .running
                            .iter()
                            .map(|(id, e)| (id.clone(), e.identifier.clone()))
                            .collect();
                        let active_states = self.dispatch_config.active_states.clone();
                        let terminal_states = self.dispatch_config.terminal_states.clone();

                        let actions = determine_reconcile_actions(
                            &running_ids,
                            &refreshed_states,
                            &active_states,
                            &terminal_states,
                            &identifiers,
                        );
                        for action in actions {
                            match action {
                                ReconcileAction::TerminateAndClean {
                                    issue_id,
                                    identifier,
                                } => {
                                    if let Some(entry) = self.state.running.get(&issue_id) {
                                        entry.cancel_token.cancel();
                                    }
                                    if let Some(entry) = self.state.running.remove(&issue_id) {
                                        self.state.codex_totals.add_runtime(entry.started_at);
                                        release_claim(&mut self.state, &issue_id);
                                        if let Some(workspace_mgr) = &self.workspace_mgr {
                                            workspace_mgr
                                                .remove_issue_workspace(&issue_id, &identifier)
                                                .await;
                                        }
                                        tracing::info!(
                                            issue_id = %issue_id,
                                            "reconciler Part B: terminated worker and cleaned workspace for terminal issue"
                                        );
                                    }
                                }
                                ReconcileAction::TerminateNoClean { issue_id, .. } => {
                                    if let Some(entry) = self.state.running.get(&issue_id) {
                                        entry.cancel_token.cancel();
                                    }
                                    if let Some(entry) = self.state.running.remove(&issue_id) {
                                        self.state.codex_totals.add_runtime(entry.started_at);
                                        release_claim(&mut self.state, &issue_id);
                                        tracing::info!(
                                            issue_id = %issue_id,
                                            "reconciler Part B: terminated worker for non-active issue"
                                        );
                                    }
                                }
                                ReconcileAction::UpdateSnapshot { .. } => {}
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "reconciler Part B: failed to fetch issue states");
                    }
                }
            }
        }

        // 5. Dispatch preflight validation (SPEC 6.3)
        if !self.preflight_validated {
            if self.tracker.is_none() {
                tracing::warn!("dispatch skipped: no tracker configured");
                self.schedule_next();
                return;
            }
            self.preflight_validated = true;
        }

        // 6. Fetch candidates from tracker
        let candidates = if let Some(tracker) = &self.tracker {
            match tracker.fetch_candidate_issues().await {
                Ok(issues) => issues,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to fetch candidate issues");
                    self.schedule_next();
                    return;
                }
            }
        } else {
            self.schedule_next();
            return;
        };

        // 7. Convert TrackerIssue → Issue and dispatch
        let issues: Vec<Issue> = candidates
            .into_iter()
            .map(|ti| Issue {
                id: ti.id,
                identifier: ti.identifier,
                title: ti.title,
                description: ti.description,
                priority: ti.priority,
                state: ti.state,
                branch_name: ti.branch_name,
                url: ti.url,
                labels: ti.labels,
                blocked_by: ti
                    .blocked_by
                    .into_iter()
                    .map(|b| crate::models::BlockerRef {
                        id: b.id,
                        identifier: b.identifier,
                        state: b.state,
                    })
                    .collect(),
                created_at: ti.created_at,
                updated_at: ti.updated_at,
            })
            .collect();

        // 8. Sort and dispatch eligible issues
        self.dispatch_and_spawn(issues).await;

        // Schedule next tick
        self.schedule_next();
    }

    /// Schedule the next tick timer.
    fn schedule_next(&mut self) {
        self.tick_timer = Some(schedule_next_tick(
            &self.event_tx,
            self.state.poll_interval_ms,
        ));
    }

    fn state_response(&self) -> StateResponse {
        let running = self
            .state
            .running
            .iter()
            .map(|(issue_id, entry)| running_row(issue_id, entry))
            .collect();

        let retrying = self.state.retry_attempts.values().map(retry_row).collect();

        StateResponse {
            generated_at: chrono::Utc::now().to_rfc3339(),
            counts: Counts {
                running: self.state.running.len(),
                retrying: self.state.retry_attempts.len(),
            },
            running,
            retrying,
            codex_totals: CodexTotalsJson {
                input_tokens: self.state.codex_totals.input_tokens,
                output_tokens: self.state.codex_totals.output_tokens,
                total_tokens: self.state.codex_totals.total_tokens,
                seconds_running: self.state.codex_totals.seconds_running_ms as f64 / 1000.0,
            },
            rate_limits: self.state.codex_rate_limits.clone(),
        }
    }

    fn issue_detail_response(&self, identifier: &str) -> Option<IssueDetailResponse> {
        if let Some((issue_id, entry)) = self
            .state
            .running
            .iter()
            .find(|(_, entry)| entry.identifier == identifier || entry.issue.id == identifier)
        {
            return Some(IssueDetailResponse {
                issue_identifier: entry.identifier.clone(),
                issue_id: issue_id.clone(),
                status: "running".to_string(),
                running: Some(running_row(issue_id, entry)),
                retry: None,
                last_error: None,
            });
        }

        self.state
            .retry_attempts
            .values()
            .find(|entry| entry.identifier == identifier || entry.issue_id == identifier)
            .map(|entry| IssueDetailResponse {
                issue_identifier: entry.identifier.clone(),
                issue_id: entry.issue_id.clone(),
                status: "retrying".to_string(),
                running: None,
                retry: Some(retry_row(entry)),
                last_error: entry.error.clone(),
            })
    }

    /// Sort candidates, check eligibility, and spawn workers for eligible issues.
    async fn dispatch_and_spawn(&mut self, mut issues: Vec<Issue>) {
        if self.state.shutting_down || issues.is_empty() {
            return;
        }

        sort_for_dispatch(&mut issues);

        for issue in issues {
            if !self.state.has_global_slots() {
                break;
            }
            if !should_dispatch(&issue, &self.state, &self.dispatch_config) {
                continue;
            }

            // Revalidate issue state before dispatch (prevent stale dispatch)
            if let Some(tracker) = &self.tracker {
                match tracker
                    .fetch_issue_states_by_ids(std::slice::from_ref(&issue.id))
                    .await
                {
                    Ok(fresh) => {
                        if let Some(fresh_issue) = fresh.first() {
                            if is_terminal_state(
                                &fresh_issue.state,
                                &self.dispatch_config.terminal_states,
                            ) {
                                tracing::info!(
                                    issue_id = %issue.id,
                                    state = %fresh_issue.state,
                                    "skipping dispatch: issue reached terminal state"
                                );
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            issue_id = %issue.id,
                            error = %e,
                            "revalidation failed, skipping dispatch"
                        );
                        continue;
                    }
                }
            }

            // Claim and spawn worker
            self.state.claimed.insert(issue.id.clone());
            self.spawn_worker(issue);
        }
    }

    /// Spawn an AgentRunner task for a dispatched issue.
    fn spawn_worker(&mut self, issue: Issue) {
        self.spawn_worker_with_attempt(issue, None);
    }

    /// Spawn an AgentRunner task for a dispatched issue with retry context.
    fn spawn_worker_with_attempt(&mut self, issue: Issue, retry_attempt: Option<u32>) {
        let issue_id = issue.id.clone();
        let issue_id_for_spawn = issue_id.clone();
        let identifier = issue.identifier.clone();
        let cancel_token = CancellationToken::new();
        let cancel_child = cancel_token.child_token();
        let event_tx = self.event_tx.clone();

        // Build AgentIssue from Issue
        let agent_issue = AgentIssue {
            id: issue.id.clone(),
            identifier: issue.identifier.clone(),
            title: issue.title.clone(),
            description: issue.description.clone(),
            priority: issue.priority,
            state: issue.state.clone(),
            labels: issue.labels.clone(),
            url: issue.url.clone(),
            branch_name: issue.branch_name.clone(),
            blocked_by: issue
                .blocked_by
                .iter()
                .map(|b| AgentBlockerRef {
                    id: b.id.clone(),
                    identifier: b.identifier.clone(),
                    state: b.state.clone(),
                })
                .collect(),
            created_at: issue.created_at.map(|d| d.to_rfc3339()),
            updated_at: issue.updated_at.map(|d| d.to_rfc3339()),
        };

        let prompt_engine = self.prompt_engine.clone();
        let workspace_mgr = self.workspace_mgr.clone();
        let config_holder = self.config_holder.clone();
        let tracker = self.tracker.clone();
        let active_states = self.dispatch_config.active_states.clone();
        let terminal_states = self.dispatch_config.terminal_states.clone();

        let worker_handle = tokio::spawn(async move {
            let Some(workspace_mgr) = workspace_mgr else {
                let _ = event_tx
                    .send(OrchestratorEvent::WorkerExitAbnormal {
                        issue_id: issue_id_for_spawn,
                        error: "no workspace manager configured".to_string(),
                    })
                    .await;
                return;
            };
            let Some(prompt_engine) = prompt_engine else {
                let _ = event_tx
                    .send(OrchestratorEvent::WorkerExitAbnormal {
                        issue_id: issue_id_for_spawn,
                        error: "no prompt engine configured".to_string(),
                    })
                    .await;
                return;
            };
            let Some(config_holder) = config_holder else {
                let _ = event_tx
                    .send(OrchestratorEvent::WorkerExitAbnormal {
                        issue_id: issue_id_for_spawn,
                        error: "no config holder configured".to_string(),
                    })
                    .await;
                return;
            };

            // Event forwarding channel: AgentRunner sends CodexEventUpdate,
            // we forward them as OrchestratorEvent::CodexUpdate
            let (codex_tx, mut codex_rx) = mpsc::channel::<CodexEventUpdate>(64);
            let issue_id_for_fwd = issue_id_for_spawn.clone();
            let event_tx_fwd = event_tx.clone();
            tokio::spawn(async move {
                while let Some(update) = codex_rx.recv().await {
                    let _ = event_tx_fwd
                        .send(OrchestratorEvent::CodexUpdate {
                            issue_id: issue_id_for_fwd.clone(),
                            update: Box::new(update),
                        })
                        .await;
                }
            });

            let runner = AgentRunner::new(
                workspace_mgr,
                config_holder,
                prompt_engine,
                codex_tx,
                cancel_child,
            );

            // Build state refresher from tracker
            let state_refresher: Arc<dyn IssueStateRefresher> = match tracker {
                Some(t) => Arc::new(TrackerStateRefresher {
                    tracker: t,
                    active_states,
                    terminal_states,
                }),
                None => Arc::new(NoopStateRefresher),
            };

            let result = runner
                .run_attempt(agent_issue, retry_attempt, state_refresher)
                .await;

            match result {
                Ok(()) => {
                    let _ = event_tx
                        .send(OrchestratorEvent::WorkerExitNormal {
                            issue_id: issue_id_for_spawn,
                        })
                        .await;
                }
                Err(e) => {
                    let _ = event_tx
                        .send(OrchestratorEvent::WorkerExitAbnormal {
                            issue_id: issue_id_for_spawn,
                            error: e.to_string(),
                        })
                        .await;
                }
            }
        });

        // Register running entry
        let session = LiveSession::new("pending".to_string(), "0".to_string());
        let entry = RunningEntry {
            worker_handle,
            cancel_token,
            identifier: identifier.clone(),
            issue: issue.clone(),
            session,
            retry_attempt,
            started_at: Instant::now(),
            started_at_utc: chrono::Utc::now(),
            cancel_sent_at: None,
        };
        self.state.running.insert(issue_id.clone(), entry);

        tracing::info!(
            issue_id = %issue_id,
            identifier = %identifier,
            attempt = ?retry_attempt,
            "spawned agent worker"
        );
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
            self.state
                .completed
                .insert(issue_id.to_string(), Instant::now());

            let tracker = match &self.tracker {
                Some(tracker) => tracker.clone(),
                None => {
                    tracing::warn!(
                        issue_id,
                        identifier = %entry.identifier,
                        "worker exited normally but no tracker is configured; not scheduling continuation"
                    );
                    release_claim(&mut self.state, issue_id);
                    return;
                }
            };

            let fresh_issues = match tracker
                .fetch_issue_states_by_ids(&[issue_id.to_string()])
                .await
            {
                Ok(issues) => issues,
                Err(e) => {
                    tracing::warn!(
                        issue_id,
                        identifier = %entry.identifier,
                        error = %e,
                        "worker exited normally but tracker revalidation failed; not scheduling continuation"
                    );
                    release_claim(&mut self.state, issue_id);
                    return;
                }
            };

            let Some(fresh_issue) = fresh_issues.into_iter().find(|i| i.id == issue_id) else {
                tracing::info!(
                    issue_id,
                    identifier = %entry.identifier,
                    "worker exited normally but issue is no longer visible; not scheduling continuation"
                );
                release_claim(&mut self.state, issue_id);
                return;
            };

            if !is_active_state(&fresh_issue.state, &self.dispatch_config.active_states) {
                tracing::info!(
                    issue_id,
                    identifier = %entry.identifier,
                    state = %fresh_issue.state,
                    "worker exited normally and issue is no longer active; not scheduling continuation"
                );
                release_claim(&mut self.state, issue_id);
                return;
            }

            tracing::info!(
                issue_id,
                identifier = %entry.identifier,
                state = %fresh_issue.state,
                "worker exited normally and issue is still active, scheduling continuation retry"
            );

            // Schedule continuation retry (fixed 1s delay)
            let delay = compute_retry_delay(1, &RetryKind::Continuation, self.max_retry_backoff_ms);
            schedule_retry(
                &mut self.state,
                RetrySchedule::new(
                    issue_id,
                    &entry.identifier,
                    1,
                    RetryKind::Continuation,
                    delay,
                    None,
                ),
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

            let delay =
                compute_retry_delay(attempt, &RetryKind::Failure, self.max_retry_backoff_ms);
            schedule_retry(
                &mut self.state,
                RetrySchedule::new(
                    issue_id,
                    &entry.identifier,
                    attempt,
                    RetryKind::Failure,
                    delay,
                    Some(error.to_string()),
                ),
                &self.event_tx,
            );
        }
    }

    /// Handle Codex event update (SPEC section 7.3).
    fn on_codex_update(&mut self, issue_id: &str, update: CodexEventUpdate) {
        if let Some(entry) = self.state.running.get_mut(issue_id) {
            // Update session activity (resets stall timer)
            entry.session.touch();

            // Update session identity from Codex events (thread_id, turn_id, session_id)
            if let Some(ref thread_id) = update.thread_id {
                entry.session.thread_id = thread_id.clone();
            }
            if let Some(ref turn_id) = update.turn_id {
                entry.session.turn_id = turn_id.clone();
            }
            if let Some(ref session_id) = update.session_id {
                entry.session.session_id = session_id.clone();
            } else if update.thread_id.is_some() || update.turn_id.is_some() {
                // Recompose session_id from thread_id + turn_id
                entry.session.session_id = crate::models::compose_session_id(
                    &entry.session.thread_id,
                    &entry.session.turn_id,
                );
            }

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
    /// Re-fetches the issue state and attempts re-dispatch.
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
            "retry timer fired, re-evaluating for dispatch"
        );

        // 2. Re-fetch issue state from tracker
        let tracker = match &self.tracker {
            Some(t) => t.clone(),
            None => {
                tracing::warn!(issue_id, "retry fired but no tracker configured");
                return;
            }
        };

        let fresh_issues = match tracker
            .fetch_issue_states_by_ids(&[issue_id.to_string()])
            .await
        {
            Ok(issues) => issues,
            Err(e) => {
                tracing::warn!(issue_id, error = %e, "retry fetch failed, rescheduling");
                let delay = compute_retry_delay(
                    entry.attempt,
                    &entry.retry_kind,
                    self.max_retry_backoff_ms,
                );
                schedule_retry(
                    &mut self.state,
                    RetrySchedule::new(
                        issue_id,
                        &entry.identifier,
                        entry.attempt,
                        entry.retry_kind,
                        delay,
                        Some(format!("retry fetch failed: {}", e)),
                    ),
                    &self.event_tx,
                );
                return;
            }
        };

        // 3. Check if issue is still active
        let fresh_issue = match fresh_issues.into_iter().find(|i| i.id == issue_id) {
            Some(i) => i,
            None => {
                tracing::info!(issue_id, "issue no longer visible, releasing claim");
                release_claim(&mut self.state, issue_id);
                return;
            }
        };

        if is_terminal_state(&fresh_issue.state, &self.dispatch_config.terminal_states) {
            tracing::info!(issue_id, state = %fresh_issue.state, "issue reached terminal state, releasing claim");

            release_claim(&mut self.state, issue_id);
            return;
        }

        if !is_active_state(&fresh_issue.state, &self.dispatch_config.active_states) {
            tracing::info!(
                issue_id,
                state = %fresh_issue.state,
                "issue is no longer active, releasing claim without retry"
            );
            release_claim(&mut self.state, issue_id);
            return;
        }

        // 4. Check if we have slots
        if !self.state.has_global_slots() {
            tracing::info!(issue_id, "no global slots available, rescheduling retry");
            let delay =
                compute_retry_delay(entry.attempt, &entry.retry_kind, self.max_retry_backoff_ms);
            schedule_retry(
                &mut self.state,
                RetrySchedule::new(
                    issue_id,
                    &entry.identifier,
                    entry.attempt,
                    entry.retry_kind,
                    delay,
                    Some("no available orchestrator slots".to_string()),
                ),
                &self.event_tx,
            );
            return;
        }

        // 5. Convert and spawn
        let issue = Issue {
            id: fresh_issue.id,
            identifier: fresh_issue.identifier,
            title: fresh_issue.title,
            description: fresh_issue.description,
            priority: fresh_issue.priority,
            state: fresh_issue.state,
            branch_name: fresh_issue.branch_name,
            url: fresh_issue.url,
            labels: fresh_issue.labels,
            blocked_by: fresh_issue
                .blocked_by
                .into_iter()
                .map(|b| crate::models::BlockerRef {
                    id: b.id,
                    identifier: b.identifier,
                    state: b.state,
                })
                .collect(),
            created_at: fresh_issue.created_at,
            updated_at: fresh_issue.updated_at,
        };

        self.spawn_worker_with_attempt(issue, Some(entry.attempt));
    }

    /// Reschedule a retry when no slots are available (called by integration layer).
    pub fn reschedule_no_slots(&mut self, issue_id: &str, identifier: &str, attempt: u32) {
        let delay =
            compute_retry_delay(attempt + 1, &RetryKind::Failure, self.max_retry_backoff_ms);
        schedule_retry(
            &mut self.state,
            RetrySchedule::new(
                issue_id,
                identifier,
                attempt + 1,
                RetryKind::Failure,
                delay,
                Some("no available orchestrator slots".to_string()),
            ),
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
    ///
    /// Workers are first cancelled via their CancellationToken, which allows them
    /// to run the after_run hook before exiting. Only after the drain timeout
    /// expires are workers force-aborted (last resort).
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

        // 4. Cancel all active workers (triggers after_run hook in each worker)
        for (_, entry) in self.state.running.iter_mut() {
            if entry.cancel_sent_at.is_none() {
                entry.cancel_token.cancel();
                entry.cancel_sent_at = Some(Instant::now());
            }
        }

        // 5. Wait for workers to exit (with timeout)
        //    Workers should run their after_run hook and exit cleanly.
        //    Only abort as a last resort after the drain timeout.
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
                        "drain timeout reached, force-killing remaining workers (after_run may not have run)"
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

fn running_row(issue_id: &str, entry: &RunningEntry) -> RunningRow {
    RunningRow {
        issue_id: issue_id.to_string(),
        issue_identifier: entry.identifier.clone(),
        state: entry.issue.state.clone(),
        session_id: entry.session.session_id.clone(),
        turn_count: entry.session.turn_count,
        last_event: entry.session.last_codex_event.clone(),
        last_message: entry.session.last_codex_message.clone(),
        started_at: entry.started_at_utc.to_rfc3339(),
        last_event_at: entry.session.last_codex_timestamp.map(|ts| ts.to_rfc3339()),
        tokens: TokensJson {
            input_tokens: entry.session.codex_input_tokens,
            output_tokens: entry.session.codex_output_tokens,
            total_tokens: entry.session.codex_total_tokens,
        },
    }
}

fn retry_row(entry: &crate::models::RetryEntry) -> RetryRow {
    let now_ms = current_monotonic_ms();
    let due_at = if entry.due_at_ms > now_ms {
        chrono::Utc::now() + chrono::Duration::milliseconds((entry.due_at_ms - now_ms) as i64)
    } else {
        chrono::Utc::now()
    };

    RetryRow {
        issue_id: entry.issue_id.clone(),
        issue_identifier: entry.identifier.clone(),
        attempt: entry.attempt,
        due_at: due_at.to_rfc3339(),
        error: entry.error.clone(),
    }
}

/// Bridge between the orchestrator's Tracker and AgentRunner's IssueStateRefresher trait.
struct TrackerStateRefresher {
    tracker: Arc<dyn Tracker>,
    active_states: Vec<String>,
    terminal_states: Vec<String>,
}

#[async_trait::async_trait]
impl IssueStateRefresher for TrackerStateRefresher {
    async fn refresh_issue_state(&self, issue_id: &str) -> Result<Option<AgentIssue>, String> {
        let issues = self
            .tracker
            .fetch_issue_states_by_ids(&[issue_id.to_string()])
            .await
            .map_err(|e| e.to_string())?;

        Ok(issues.first().map(tracker_issue_to_agent_issue))
    }

    fn is_terminal_state(&self, state: &str) -> bool {
        self.terminal_states
            .iter()
            .any(|s| s.eq_ignore_ascii_case(state))
    }

    fn is_active_state(&self, state: &str) -> bool {
        self.active_states
            .iter()
            .any(|s| s.eq_ignore_ascii_case(state))
    }
}

/// Fallback when no tracker is configured — issue is always considered active.
struct NoopStateRefresher;

#[async_trait::async_trait]
impl IssueStateRefresher for NoopStateRefresher {
    async fn refresh_issue_state(&self, _issue_id: &str) -> Result<Option<AgentIssue>, String> {
        Ok(None)
    }
    fn is_terminal_state(&self, _state: &str) -> bool {
        false
    }
    fn is_active_state(&self, _state: &str) -> bool {
        true
    }
}

fn tracker_issue_to_agent_issue(ti: &TrackerIssue) -> AgentIssue {
    AgentIssue {
        id: ti.id.clone(),
        identifier: ti.identifier.clone(),
        title: ti.title.clone(),
        description: ti.description.clone(),
        priority: ti.priority,
        state: ti.state.clone(),
        labels: ti.labels.clone(),
        url: ti.url.clone(),
        branch_name: ti.branch_name.clone(),
        blocked_by: ti
            .blocked_by
            .iter()
            .map(|b| AgentBlockerRef {
                id: b.id.clone(),
                identifier: b.identifier.clone(),
                state: b.state.clone(),
            })
            .collect(),
        created_at: ti.created_at.map(|d| d.to_rfc3339()),
        updated_at: ti.updated_at.map(|d| d.to_rfc3339()),
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

        let issues = vec![Issue {
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
        }];

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

        let issues = vec![Issue {
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
        }];

        orch.dispatch_candidates(issues);
        assert!(!orch.state.claimed.contains("1"));
    }
}

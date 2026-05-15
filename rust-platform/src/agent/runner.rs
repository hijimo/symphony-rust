//! Agent Runner — full worker lifecycle for a single issue.
//!
//! Orchestrates workspace preparation, prompt construction, Codex session
//! management, and the multi-turn loop for one issue attempt.
//!
//! SPEC reference: Section 10.7, Section 16.5

use std::sync::Arc;

use thiserror::Error;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::service_config::ServiceConfig;
use crate::config::watcher::ConfigHolder;
use crate::prompt::{BlockerContext, IssueContext, PromptEngine};
use crate::workspace::{WorkspaceError, WorkspaceManager};

use super::codex_client::{CodexClient, CodexError, CodexEventUpdate, TurnResult};

/// Errors from agent runner operations.
#[derive(Debug, Error)]
pub enum AgentError {
    #[error("workspace error: {0}")]
    Workspace(#[from] WorkspaceError),

    #[error("codex error: {0}")]
    Codex(#[from] CodexError),

    #[error("prompt render error: {0}")]
    PromptRender(String),

    #[error("agent run cancelled")]
    Cancelled,

    #[error("issue state refresh failed: {0}")]
    IssueStateRefreshFailed(String),

    #[error("max turns ({max_turns}) exhausted")]
    MaxTurnsExhausted { max_turns: u32 },
}

/// Issue data passed to the agent runner.
///
/// This is a simplified issue model for the agent layer, decoupled from
/// the platform-specific Issue type.
#[derive(Debug, Clone)]
pub struct AgentIssue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub description: Option<String>,
    pub priority: Option<i32>,
    pub state: String,
    pub labels: Vec<String>,
    pub url: Option<String>,
    pub branch_name: Option<String>,
    pub blocked_by: Vec<AgentBlockerRef>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// Blocker reference in the agent layer.
#[derive(Debug, Clone)]
pub struct AgentBlockerRef {
    pub id: Option<String>,
    pub identifier: Option<String>,
    pub state: Option<String>,
}

impl AgentIssue {
    /// Convert to prompt rendering context.
    pub fn to_prompt_context(&self) -> IssueContext {
        IssueContext {
            id: self.id.clone(),
            identifier: self.identifier.clone(),
            title: self.title.clone(),
            description: self.description.clone(),
            priority: self.priority,
            state: self.state.clone(),
            labels: self.labels.clone(),
            url: self.url.clone(),
            branch_name: self.branch_name.clone(),
            blocked_by: self
                .blocked_by
                .iter()
                .map(|b| BlockerContext {
                    id: b.id.clone(),
                    identifier: b.identifier.clone(),
                    state: b.state.clone(),
                })
                .collect(),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
        }
    }
}

/// Callback trait for refreshing issue state from the tracker.
///
/// The agent runner calls this after each turn to check if the issue
/// is still active and should continue processing.
#[async_trait::async_trait]
pub trait IssueStateRefresher: Send + Sync {
    /// Fetch the current state of an issue by ID.
    /// Returns None if the issue cannot be found.
    async fn refresh_issue_state(&self, issue_id: &str) -> Result<Option<AgentIssue>, String>;

    /// Check if a state is terminal.
    fn is_terminal_state(&self, state: &str) -> bool;

    /// Check if a state is active.
    fn is_active_state(&self, state: &str) -> bool;
}

/// Agent Runner — manages the full worker lifecycle for one issue.
///
/// Responsibilities:
/// 1. Snapshot config (isolation from hot reload)
/// 2. Ensure workspace exists
/// 3. Run before_run hook
/// 4. Start CodexClient
/// 5. Turn loop (up to max_turns)
/// 6. Each turn: check cancel, build prompt, run_turn, refresh issue state
/// 7. Stop client, run after_run hook
pub struct AgentRunner {
    workspace_mgr: Arc<WorkspaceManager>,
    config_holder: Arc<ConfigHolder>,
    prompt_engine: Arc<PromptEngine>,
    event_tx: mpsc::Sender<CodexEventUpdate>,
    cancel_token: CancellationToken,
}

impl AgentRunner {
    /// Create a new AgentRunner.
    pub fn new(
        workspace_mgr: Arc<WorkspaceManager>,
        config_holder: Arc<ConfigHolder>,
        prompt_engine: Arc<PromptEngine>,
        event_tx: mpsc::Sender<CodexEventUpdate>,
        cancel_token: CancellationToken,
    ) -> Self {
        Self {
            workspace_mgr,
            config_holder,
            prompt_engine,
            event_tx,
            cancel_token,
        }
    }

    /// Execute one complete worker attempt (SPEC Section 16.5).
    ///
    /// This is the main entry point for a worker task. It handles the full
    /// lifecycle from workspace preparation through turn execution to cleanup.
    pub async fn run_attempt(
        &self,
        issue: AgentIssue,
        attempt: Option<u32>,
        state_refresher: Arc<dyn IssueStateRefresher>,
    ) -> Result<(), AgentError> {
        // 0. Snapshot config at start (isolation from hot reload)
        let config_snapshot = self.config_holder.load();
        let service_config = &config_snapshot.service;

        tracing::info!(
            issue_id = %issue.id,
            identifier = %issue.identifier,
            attempt = ?attempt,
            "starting agent run attempt"
        );

        // 1. Ensure workspace exists
        let ws = self.workspace_mgr.ensure_workspace(&issue.identifier).await?;
        tracing::info!(
            identifier = %issue.identifier,
            path = %ws.path.display(),
            created_now = ws.created_now,
            "workspace ready"
        );

        // 2. Run before_run hook (failure is fatal)
        self.workspace_mgr.run_before_run(&ws.path).await?;

        // 3. Start Codex app-server client
        let mut client = match CodexClient::start(
            &ws.path,
            &service_config.codex,
            self.cancel_token.child_token(),
        )
        .await
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(
                    issue_id = %issue.id,
                    error = %e,
                    "failed to start codex client"
                );
                self.workspace_mgr.run_after_run(&ws.path).await;
                return Err(AgentError::Codex(e));
            }
        };

        // 4. Turn loop
        let max_turns = service_config.max_turns;
        let mut turn_number = 1u32;
        let mut current_issue = issue.clone();

        let result = self
            .run_turn_loop(
                &mut client,
                &mut current_issue,
                attempt,
                max_turns,
                &mut turn_number,
                &state_refresher,
                service_config,
            )
            .await;

        // 5. Stop client and run after_run hook
        client.stop().await;
        self.workspace_mgr.run_after_run(&ws.path).await;

        match result {
            Ok(()) => {
                tracing::info!(
                    issue_id = %issue.id,
                    identifier = %issue.identifier,
                    turns_completed = turn_number,
                    "agent run completed successfully"
                );
                Ok(())
            }
            Err(e) => {
                tracing::error!(
                    issue_id = %issue.id,
                    identifier = %issue.identifier,
                    error = %e,
                    "agent run failed"
                );
                Err(e)
            }
        }
    }

    /// Internal turn loop implementation.
    async fn run_turn_loop(
        &self,
        client: &mut CodexClient,
        current_issue: &mut AgentIssue,
        attempt: Option<u32>,
        max_turns: u32,
        turn_number: &mut u32,
        state_refresher: &Arc<dyn IssueStateRefresher>,
        _service_config: &ServiceConfig,
    ) -> Result<(), AgentError> {
        loop {
            // Check cancellation before each turn
            if self.cancel_token.is_cancelled() {
                tracing::info!(
                    issue_id = %current_issue.id,
                    turn = *turn_number,
                    "agent run cancelled"
                );
                return Err(AgentError::Cancelled);
            }

            // Build prompt for this turn
            let prompt_context = current_issue.to_prompt_context();
            let prompt = self
                .prompt_engine
                .render(&prompt_context, attempt, *turn_number, max_turns)
                .map_err(|e| AgentError::PromptRender(e.to_string()))?;

            tracing::info!(
                issue_id = %current_issue.id,
                turn = *turn_number,
                max_turns,
                "executing turn"
            );

            // Execute the turn
            let turn_result = client
                .run_turn(&prompt, &current_issue.id, &self.event_tx)
                .await?;

            // Handle turn result
            match turn_result {
                TurnResult::Completed => {
                    tracing::info!(
                        issue_id = %current_issue.id,
                        turn = *turn_number,
                        "turn completed successfully"
                    );
                }
                TurnResult::Failed { reason } => {
                    tracing::warn!(
                        issue_id = %current_issue.id,
                        turn = *turn_number,
                        reason = %reason,
                        "turn failed"
                    );
                    return Err(AgentError::Codex(CodexError::TurnFailed { reason }));
                }
                TurnResult::InputRequired => {
                    tracing::warn!(
                        issue_id = %current_issue.id,
                        turn = *turn_number,
                        "turn requires user input (not supported)"
                    );
                    return Err(AgentError::Codex(CodexError::TurnInputRequired));
                }
            }

            // Refresh issue state after turn (SPEC Section 16.5 REQUIRED)
            match state_refresher
                .refresh_issue_state(&current_issue.id)
                .await
            {
                Err(e) => {
                    tracing::error!(
                        issue_id = %current_issue.id,
                        error = %e,
                        "failed to refresh issue state"
                    );
                    return Err(AgentError::IssueStateRefreshFailed(e));
                }
                Ok(None) => {
                    // Issue not found — treat as no longer active
                    tracing::info!(
                        issue_id = %current_issue.id,
                        "issue not found during state refresh, ending run"
                    );
                    return Ok(());
                }
                Ok(Some(updated)) => {
                    // Check if issue is still active
                    if state_refresher.is_terminal_state(&updated.state)
                        || !state_refresher.is_active_state(&updated.state)
                    {
                        tracing::info!(
                            issue_id = %current_issue.id,
                            new_state = %updated.state,
                            "issue no longer active, ending run"
                        );
                        return Ok(());
                    }
                    *current_issue = updated;
                }
            }

            // Check max turns
            if *turn_number >= max_turns {
                tracing::info!(
                    issue_id = %current_issue.id,
                    max_turns,
                    "max turns reached"
                );
                return Ok(());
            }

            *turn_number += 1;
        }
    }
}

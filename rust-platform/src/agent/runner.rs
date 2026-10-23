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

use crate::config::repos::{RepoRole, RepoSpec};
use crate::config::watcher::ConfigHolder;
use crate::prompt::{BlockerContext, IssueContext, PromptEngine, RepoContext};
use crate::workspace::repo_api::{GitlabRepoApi, RepoApi};
use crate::workspace::repos::{RepoCheckoutContext, RepoState};
use crate::workspace::{MultiRepoPrepare, WorkspaceError, WorkspaceManager, WorkspaceRunLease};

use super::codex_client::{CodexClient, CodexError, TurnResult};
use crate::models::CodexEventUpdate;

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

    #[error("read-only repository modified: {0}")]
    ReadOnlyViolation(String),
}

/// Build prompt repo contexts from config + marker state (design §6.1).
fn repo_contexts(
    specs: &[RepoSpec],
    states: &std::collections::BTreeMap<String, RepoState>,
) -> Vec<RepoContext> {
    specs
        .iter()
        .map(|spec| {
            let state = states.get(&spec.alias);
            RepoContext {
                alias: spec.alias.clone(),
                slug: spec.slug.clone(),
                path: state
                    .map(|s| s.path.clone())
                    .unwrap_or_else(|| spec.alias.clone()),
                role: spec.role.as_str().to_string(),
                writable: spec.writable(),
                branch: state.and_then(|s| s.branch.clone()),
                default_branch: spec.default_branch.clone(),
                merge_after: spec.merge_after.clone(),
            }
        })
        .collect()
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

    /// Move the issue to `state` (used to hand off to humans after a
    /// read-only integrity violation). Default: unsupported.
    async fn set_issue_state(&self, _issue_id: &str, _state: &str) -> Result<(), String> {
        Err("state writes not supported by this refresher".to_string())
    }
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
    /// Multi-repo remote API (Rework label events, MR close). `None` in
    /// single-repo mode or when credentials are unavailable.
    repo_api: Option<Arc<dyn RepoApi>>,
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
            repo_api: None,
            workspace_mgr,
            config_holder,
            prompt_engine,
            event_tx,
            cancel_token,
        }
    }

    /// Inject the multi-repo remote API (tests use a fake).
    pub fn with_repo_api(mut self, api: Arc<dyn RepoApi>) -> Self {
        self.repo_api = Some(api);
        self
    }

    /// Build the checkout context from the process environment
    /// (`GITLAB_HOST` / `GITLAB_TOKEN`, injected by web-platform at spawn).
    fn checkout_context(&self, issue: &AgentIssue) -> RepoCheckoutContext {
        let platform_host = std::env::var("GITLAB_HOST").unwrap_or_else(|_| {
            let endpoint = self.config_holder.load().service.tracker_endpoint.clone();
            endpoint
                .trim_end_matches('/')
                .trim_end_matches("/api/v4")
                .to_string()
        });
        let root = self.workspace_mgr.root().to_path_buf();
        RepoCheckoutContext {
            platform_host,
            token: std::env::var("GITLAB_TOKEN").ok().filter(|t| !t.is_empty()),
            // Outside every issue workspace (design §5.2).
            askpass_dir: root.join(".symphony").join("askpass"),
            issue_number: issue.identifier.trim_start_matches('#').to_string(),
            clone_timeout: std::time::Duration::from_secs(15 * 60),
            fetch_timeout: std::time::Duration::from_secs(3 * 60),
        }
    }

    fn repo_api_for(
        &self,
        ctx: &RepoCheckoutContext,
        primary_slug: &str,
    ) -> Option<Arc<dyn RepoApi>> {
        if let Some(api) = &self.repo_api {
            return Some(api.clone());
        }
        let token = ctx.token.as_deref()?;
        let api_base = format!("{}/api/v4", ctx.platform_host.trim_end_matches('/'));
        GitlabRepoApi::new(&api_base, token, primary_slug)
            .map(|a| Arc::new(a) as Arc<dyn RepoApi>)
            .map_err(|e| tracing::warn!(error = %e, "failed to build multi-repo API client"))
            .ok()
    }

    /// Send a liveness ping so long checkouts are not mistaken for a stall.
    fn activity_ping(&self) -> impl Fn() + Send + Sync + 'static {
        let tx = self.event_tx.clone();
        let last = Arc::new(std::sync::Mutex::new(
            std::time::Instant::now() - std::time::Duration::from_secs(60),
        ));
        move || {
            let mut guard = match last.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            // git_with_progress ticks every 30s; throttle slightly below that
            // so each tick is forwarded (stall timeout defaults to 300s).
            if guard.elapsed() >= std::time::Duration::from_secs(25) {
                *guard = std::time::Instant::now();
                let _ = tx.try_send(CodexEventUpdate {
                    event_type: Some("workspace.checkout".to_string()),
                    message: Some("repository checkout in progress".to_string()),
                    timestamp: Some(chrono::Utc::now()),
                    ..CodexEventUpdate::default()
                });
            }
        }
    }

    /// Prepare a multi-repo workspace, apply a pending Rework, and return the
    /// lease plus the repo contexts for the prompt.
    async fn prepare_multi_repo(
        &self,
        issue: &AgentIssue,
        specs: &[RepoSpec],
        run_id: &str,
        service_instance_id: &str,
    ) -> Result<(WorkspaceRunLease, RepoCheckoutContext, Vec<RepoContext>), AgentError> {
        let ctx = self.checkout_context(issue);
        let ping = self.activity_ping();
        let (lease, mut state) = self
            .workspace_mgr
            .prepare_multi_repo_workspace(MultiRepoPrepare {
                issue_id: &issue.id,
                issue_identifier: &issue.identifier,
                run_id,
                service_instance_id,
                checkout: &ctx,
                specs,
                on_progress: &ping,
            })
            .await?;

        if issue.state.eq_ignore_ascii_case("rework") {
            let primary = specs
                .iter()
                .find(|s| s.role == RepoRole::Primary)
                .unwrap_or(&specs[0]);
            match self.repo_api_for(&ctx, &primary.slug) {
                Some(api) => match api
                    .latest_label_add_event(&ctx.issue_number, "Rework")
                    .await
                {
                    Ok(Some(event_id)) => {
                        state = self
                            .workspace_mgr
                            .apply_rework(&lease, &ctx, specs, event_id, api.as_ref())
                            .await?;
                    }
                    Ok(None) => {
                        tracing::warn!(issue_id = %issue.id, "Rework state without an add-label event; keeping branches")
                    }
                    Err(e) => {
                        return Err(AgentError::Workspace(WorkspaceError::RepoCheckout {
                            reason: format!("failed to read Rework label events: {e}"),
                        }))
                    }
                },
                None => {
                    tracing::warn!(issue_id = %issue.id, "no GitLab credentials; Rework branch switch skipped")
                }
            }
        }

        let contexts = repo_contexts(specs, &state.repos);
        Ok((lease, ctx, contexts))
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

        // 1. Prepare an issue-id keyed workspace and hold the issue lock for
        // the whole attempt.
        let run_id = format!(
            "{}-{}-{}",
            WorkspaceManager::issue_id_path_key(&issue.id),
            attempt.unwrap_or(0),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let service_instance_id = std::env::var("SYMPHONY_SERVICE_INSTANCE_ID")
            .unwrap_or_else(|_| format!("standalone-{}", std::process::id()));
        let mut specs = service_config.workspace_repos.clone();
        if !specs.is_empty()
            && self
                .workspace_mgr
                .has_legacy_workspace(&issue.id, &issue.identifier)
                .await
        {
            tracing::info!(
                issue_id = %issue.id,
                identifier = %issue.identifier,
                "existing single-repo workspace; continuing in single-repo layout (not migrated)"
            );
            specs.clear();
        }
        let (lease, multi) = if specs.is_empty() {
            let lease = self
                .workspace_mgr
                .prepare_issue_workspace(
                    &issue.id,
                    &issue.identifier,
                    &run_id,
                    &service_instance_id,
                )
                .await?;
            (lease, None)
        } else {
            let (lease, ctx, repos) = self
                .prepare_multi_repo(&issue, &specs, &run_id, &service_instance_id)
                .await?;
            (lease, Some((ctx, repos)))
        };
        let repo_contexts: Vec<RepoContext> =
            multi.as_ref().map(|(_, r)| r.clone()).unwrap_or_default();
        let ws = &lease.workspace;
        tracing::info!(
            issue_id = %issue.id,
            identifier = %issue.identifier,
            path = %ws.path.display(),
            workspace_key = %ws.workspace_key,
            issue_id_path_key = %lease.issue_id_path_key,
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
                &repo_contexts,
            )
            .await;

        // 5. Stop client and run after_run hook
        client.stop().await;
        self.workspace_mgr.run_after_run(&ws.path).await;

        // 5b. Read-only integrity check *after* the agent ran (design §5.3).
        if let Some((ctx, _)) = &multi {
            let violations = self
                .workspace_mgr
                .readonly_violations(&lease, ctx, &specs)
                .await;
            if !violations.is_empty() {
                tracing::error!(
                    issue_id = %issue.id,
                    identifier = %issue.identifier,
                    violations = ?violations,
                    "read-only repository modified; handing off to Human Review"
                );
                if let Err(e) = state_refresher
                    .set_issue_state(&issue.id, "Human Review")
                    .await
                {
                    tracing::error!(issue_id = %issue.id, error = %e, "failed to move issue to Human Review");
                }
                return Err(AgentError::ReadOnlyViolation(violations.join("; ")));
            }
        }

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
    #[allow(clippy::too_many_arguments)]
    async fn run_turn_loop(
        &self,
        client: &mut CodexClient,
        current_issue: &mut AgentIssue,
        attempt: Option<u32>,
        max_turns: u32,
        turn_number: &mut u32,
        state_refresher: &Arc<dyn IssueStateRefresher>,
        repos: &[RepoContext],
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
                .render_with_repos(&prompt_context, attempt, *turn_number, max_turns, repos)
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
            match state_refresher.refresh_issue_state(&current_issue.id).await {
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
                return Err(AgentError::MaxTurnsExhausted { max_turns });
            }

            *turn_number += 1;
        }
    }
}

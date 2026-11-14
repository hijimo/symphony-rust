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
use crate::workspace::{
    MultiRepoPrepare, SingleRootPrepare, WorkspaceError, WorkspaceLayout, WorkspaceManager,
    WorkspaceRunLease,
};

use super::codex_client::{CodexClient, CodexError, CodexStartOptions, TurnResult};
use super::{post_run, repo_instructions, validation};
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

    /// Whether the workpad / label operations below are available.
    fn supports_workpad(&self) -> bool {
        false
    }

    /// Read the workpad comment: `(comment_id, body)`.
    async fn fetch_workpad(&self, _issue_id: &str) -> Result<Option<(u64, String)>, String> {
        Err("workpad not supported".to_string())
    }

    /// Replace the workpad comment body.
    async fn update_workpad(&self, _comment_id: u64, _body: &str) -> Result<(), String> {
        Err("workpad not supported".to_string())
    }

    /// Add plain labels.
    async fn add_labels(&self, _issue_id: &str, _labels: &[String]) -> Result<(), String> {
        Err("labels not supported".to_string())
    }

    /// Remove plain labels.
    async fn remove_labels(&self, _issue_id: &str, _labels: &[String]) -> Result<(), String> {
        Err("labels not supported".to_string())
    }

    /// List files in a repository directory (see `Tracker::list_repo_directory`).
    async fn list_repo_directory(
        &self,
        _slug: &str,
        _path: &str,
        _git_ref: &str,
    ) -> Result<Option<Vec<crate::platform::RepoFile>>, String> {
        Err("repository listing not supported".to_string())
    }

    /// Open PR/MR for `branch` in repository `slug`.
    async fn find_open_pull_request(
        &self,
        _slug: &str,
        _branch: &str,
    ) -> Result<Option<crate::platform::PullRequestRef>, String> {
        Err("pull requests not supported".to_string())
    }

    /// User comments on a PR/MR.
    async fn list_pull_request_comments(
        &self,
        _pr: &crate::platform::PullRequestRef,
    ) -> Result<Vec<crate::platform::Comment>, String> {
        Err("pull requests not supported".to_string())
    }

    /// Write a marker-identified PR/MR comment.
    async fn upsert_pull_request_comment(
        &self,
        _pr: &crate::platform::PullRequestRef,
        _marker: &str,
        _body: &str,
        _mode: crate::platform::CommentMode,
    ) -> Result<(), String> {
        Err("pull requests not supported".to_string())
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
    ) -> Result<
        (
            WorkspaceRunLease,
            RepoCheckoutContext,
            Vec<RepoContext>,
            std::collections::BTreeSet<String>,
        ),
        AgentError,
    > {
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
        // Repositories whose refresh failed this run (stale `origin/<base>`).
        let stale = state
            .repos
            .iter()
            .filter(|(_, r)| r.last_error.is_some())
            .map(|(alias, _)| alias.clone())
            .collect();
        Ok((lease, ctx, contexts, stale))
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
        // the whole attempt. Layout: multi-repo, single_root (Symphony checks
        // the repository out at the root) or legacy (agent-cloned).
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
        let single_spec = service_config.workspace_single_repo.clone();
        let mut stale_repos = std::collections::BTreeSet::new();
        let (lease, multi, layout) = if specs.is_empty() {
            let single_root = match &single_spec {
                Some(spec) => {
                    let ctx = self.checkout_context(&issue);
                    let ping = self.activity_ping();
                    self.workspace_mgr
                        .prepare_single_root_workspace(SingleRootPrepare {
                            issue_id: &issue.id,
                            issue_identifier: &issue.identifier,
                            run_id: &run_id,
                            service_instance_id: &service_instance_id,
                            checkout: &ctx,
                            spec,
                            on_progress: &ping,
                        })
                        .await?
                }
                None => None,
            };
            match single_root {
                Some(lease) => (lease, None, WorkspaceLayout::SingleRoot),
                None => {
                    let lease = self
                        .workspace_mgr
                        .prepare_issue_workspace(
                            &issue.id,
                            &issue.identifier,
                            &run_id,
                            &service_instance_id,
                        )
                        .await?;
                    (lease, None, WorkspaceLayout::Legacy)
                }
            }
        } else {
            let (lease, ctx, repos, stale) = self
                .prepare_multi_repo(&issue, &specs, &run_id, &service_instance_id)
                .await?;
            stale_repos = stale;
            (lease, Some((ctx, repos)), WorkspaceLayout::MultiRepo)
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

        // 2b. Self-verification pre-run (design §3.3–§3.5, §4.3).
        let single_default = single_spec.as_ref().map(|s| s.default_branch.as_str());
        let checked = post_run::writable_repos(&ws.path, layout, &repo_contexts, single_default);
        self.pre_run_cleanup(&issue, &state_refresher).await;
        self.instruction_guard(&issue, layout, &checked, &state_refresher)
            .await;
        let root = self.workspace_mgr.root().to_path_buf();
        validation::cleanup_e2e_for_issue(&root, &lease.issue_id_path_key, &run_id);
        let mut env = vec![("SYMPHONY_RUN_ID".to_string(), run_id.clone())];
        match validation::write_e2e_env(&root, &run_id, issue.description.as_deref()) {
            Ok(Some((path, problems))) if problems.is_empty() => {
                env.push((
                    "SYMPHONY_E2E_ENV_FILE".to_string(),
                    path.to_string_lossy().to_string(),
                ));
            }
            Ok(Some((_, problems))) => {
                tracing::warn!(issue_id = %issue.id, problems = ?problems, "E2E section rejected; no env file");
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(issue_id = %issue.id, error = %e, "failed to write E2E env file")
            }
        }
        let instructions = self
            .repo_instructions(
                &ws.path,
                layout,
                &repo_contexts,
                &issue,
                single_spec.as_ref(),
                &stale_repos,
            )
            .await;

        // 3. Start Codex app-server client
        let mut client = match CodexClient::start_with(
            &ws.path,
            &service_config.codex,
            self.cancel_token.child_token(),
            CodexStartOptions {
                env,
                developer_instructions: Some(post_run::SAFETY_RULES.to_string()),
                read_only: false,
            },
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
                validation::remove_e2e_run(&root, &run_id);
                return Err(AgentError::Codex(e));
            }
        };

        // Approved change documents on the default branch (P2 design §4 B3).
        let change_docs = match super::change_docs::doc_target(
            &issue.identifier,
            &issue.id,
            &repo_contexts,
            single_spec.as_ref(),
            &service_config.tracker_project_slug,
        ) {
            Some(target) => super::change_docs::lookup(state_refresher.as_ref(), &target).await,
            None => crate::prompt::ChangeDocsLookup::Unknown,
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
                instructions.as_deref(),
                &run_id,
                &change_docs,
                layout,
            )
            .await;

        // 5. Stop client and run after_run hook
        client.stop().await;
        self.workspace_mgr.run_after_run(&ws.path).await;
        validation::remove_e2e_run(&root, &run_id);

        self.instruction_guard(&issue, layout, &checked, &state_refresher)
            .await;

        // 5b. Blocked-marker recognition and the optional validation gate
        // (design §3.5). Runs for every outcome, including cancelled runs
        // and read-only violations, so a blocked marker is never lost.
        self.post_run_checks(
            &issue,
            &run_id,
            layout,
            &checked,
            service_config.validation_gate,
            &state_refresher,
        )
        .await;

        // 5b'. Plan deviation check (P2 design §4 B2); annotates only.
        // Legacy workspaces are cloned by the agent during the run, so the
        // repository location is only known now.
        let plan_repos = if layout == WorkspaceLayout::Legacy {
            post_run::writable_repos(&ws.path, layout, &repo_contexts, None)
        } else {
            checked.clone()
        };
        let plan_report = self
            .plan_deviation_check(
                &issue,
                layout,
                &plan_repos,
                &repo_contexts,
                single_spec.as_ref(),
                &service_config.tracker_project_slug,
                &state_refresher,
            )
            .await;

        // 5b''. AI review of the PR before human review (P2 design §4 B1).
        if service_config.review.enabled {
            self.ai_review_check(
                &issue,
                layout,
                &plan_repos,
                &repo_contexts,
                single_spec.as_ref(),
                service_config,
                &change_docs,
                plan_report.as_deref(),
                &state_refresher,
            )
            .await;
        }

        // 5c. Read-only integrity check *after* the agent ran (design §5.3).
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

    /// Run-start cleanup (design §3.5): for runs starting in Todo / In Progress
    /// / Rework, remove the self-verification labels and rewrite old blocked
    /// markers to `blocked-cleared` before the agent starts.
    async fn pre_run_cleanup(&self, issue: &AgentIssue, refresher: &Arc<dyn IssueStateRefresher>) {
        if !refresher.supports_workpad() || !starts_development(&issue.state) {
            return;
        }
        let labels: Vec<String> = validation::ALL_LABELS
            .iter()
            .map(|l| l.to_string())
            .collect();
        if let Err(e) = refresher.remove_labels(&issue.id, &labels).await {
            tracing::warn!(issue_id = %issue.id, error = %e, "failed to remove self-verification labels");
        }
        // If old blocked markers cannot be cleared they could exempt this
        // run from the gate, so flag the issue instead (design §3.5).
        let flag_missing = || async {
            let _ = refresher
                .add_labels(
                    &issue.id,
                    &[validation::LABEL_VALIDATION_MISSING.to_string()],
                )
                .await;
        };
        match refresher.fetch_workpad(&issue.id).await {
            Ok(Some((comment_id, body))) => {
                if let Some(cleared) = validation::clear_blocked_markers(&body) {
                    if let Err(e) = refresher.update_workpad(comment_id, &cleared).await {
                        tracing::warn!(issue_id = %issue.id, error = %e, "failed to clear blocked markers");
                        flag_missing().await;
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(issue_id = %issue.id, error = %e, "failed to read workpad at run start");
                flag_missing().await;
            }
        }
    }

    /// Single-repo working-branch guard (design §4.3): in single_root Codex
    /// loads `AGENTS.md` / `.codex/` from the working tree, which the agent
    /// can edit. Changes introduced by this branch are never reverted; they
    /// are flagged in the workpad so reviewers check them.
    async fn instruction_guard(
        &self,
        issue: &AgentIssue,
        layout: WorkspaceLayout,
        repos: &[post_run::CheckedRepo],
        refresher: &Arc<dyn IssueStateRefresher>,
    ) {
        if layout != WorkspaceLayout::SingleRoot || !refresher.supports_workpad() {
            return;
        }
        let Some(repo) = repos.first() else { return };
        let files = match post_run::instruction_changes(&repo.dir, &repo.default_branch).await {
            Ok(f) if f.is_empty() => return,
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(issue_id = %issue.id, error = %e, "instruction-file guard skipped");
                return;
            }
        };
        tracing::warn!(issue_id = %issue.id, files = ?files, "branch changes instruction files");
        match refresher.fetch_workpad(&issue.id).await {
            Ok(Some((comment_id, body))) => {
                if let Some(updated) = post_run::with_instruction_notice(&body, &files) {
                    if let Err(e) = refresher.update_workpad(comment_id, &updated).await {
                        tracing::warn!(issue_id = %issue.id, error = %e, "failed to write instruction-file notice");
                    }
                }
            }
            // No workpad yet: the agent creates it; the next check writes the notice.
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(issue_id = %issue.id, error = %e, "failed to read workpad for instruction-file notice")
            }
        }
    }

    /// AGENTS.md block for layouts where Codex cannot load it by itself
    /// (multi-repo and legacy `repo/`). `None` for single_root.
    async fn repo_instructions(
        &self,
        workspace: &std::path::Path,
        layout: WorkspaceLayout,
        repos: &[RepoContext],
        issue: &AgentIssue,
        single: Option<&crate::config::service_config::SingleRepoSpec>,
        stale: &std::collections::BTreeSet<String>,
    ) -> Option<String> {
        let sources: Vec<repo_instructions::RepoInstructionSource> = match layout {
            WorkspaceLayout::SingleRoot => return None,
            WorkspaceLayout::MultiRepo => repos
                .iter()
                .map(|r| repo_instructions::RepoInstructionSource {
                    path: r.path.clone(),
                    slug: r.slug.clone(),
                    writable: r.writable,
                    default_branch: r.default_branch.clone(),
                    refreshed: !stale.contains(&r.alias),
                })
                .collect(),
            WorkspaceLayout::Legacy => {
                // Agent-cloned at the root: Codex loads it itself.
                if workspace.join(".git").exists() || !workspace.join("repo").join(".git").exists()
                {
                    return None;
                }
                let spec = single?;
                let ctx = self.checkout_context(issue);
                let refreshed = match crate::workspace::repos::refresh_legacy_repo(
                    &ctx,
                    &workspace.join("repo"),
                    &spec.slug,
                    &spec.default_branch,
                )
                .await
                {
                    Ok(_) => true,
                    Err(e) => {
                        tracing::warn!(error = %e, "legacy repo/: refreshing default branch failed");
                        false
                    }
                };
                vec![repo_instructions::RepoInstructionSource {
                    path: "repo".to_string(),
                    slug: spec.slug.clone(),
                    writable: true,
                    default_branch: spec.default_branch.clone(),
                    refreshed,
                }]
            }
        };
        repo_instructions::build(workspace, &sources, &repo_instructions::new_nonce()).await
    }

    /// Blocked-marker recognition (always) and the validation gate (when
    /// enabled), after the agent stopped (design §3.5).
    async fn post_run_checks(
        &self,
        issue: &AgentIssue,
        run_id: &str,
        layout: WorkspaceLayout,
        repos: &[post_run::CheckedRepo],
        gate_enabled: bool,
        refresher: &Arc<dyn IssueStateRefresher>,
    ) {
        if !refresher.supports_workpad() {
            return;
        }
        let state = match refresher.refresh_issue_state(&issue.id).await {
            Ok(Some(i)) => Some(i.state),
            _ => None,
        };
        // Humans re-scheduled the issue meanwhile: leave labels alone.
        if state
            .as_deref()
            .is_some_and(|s| is_state(s, &["todo", "rework"]))
        {
            return;
        }
        let workpad = match refresher.fetch_workpad(&issue.id).await {
            Ok(Some((_, body))) => body,
            Ok(None) => String::new(),
            Err(e) => {
                tracing::warn!(issue_id = %issue.id, error = %e, "failed to read workpad after run; skipping blocked recognition");
                let _ = refresher
                    .add_labels(
                        &issue.id,
                        &[validation::LABEL_VALIDATION_MISSING.to_string()],
                    )
                    .await;
                return;
            }
        };
        let heads = post_run::current_heads(repos).await;
        let markers = validation::blocked_markers(&workpad);
        if let Some(marker) = validation::effective_blocked_marker(&markers, run_id, &heads) {
            tracing::info!(issue_id = %issue.id, reason = %marker.reason, "blocked marker recognized");
            if let Err(e) = refresher
                .add_labels(&issue.id, &[marker.label().to_string()])
                .await
            {
                tracing::warn!(issue_id = %issue.id, error = %e, "failed to add blocked label");
            }
            return; // exempt from the gate
        }
        // A marker that is not effective must not vanish silently: either
        // this run wrote one Symphony cannot verify (HEAD of a writable
        // repository unreadable, heads or run id wrong), or - for runs that
        // started in development, whose old markers were cleared before the
        // agent started - any remaining active marker was written this run.
        let unverifiable = (heads.len() < repos.len() && markers.iter().any(|m| m.run == run_id))
            || (starts_development(&issue.state) && !markers.is_empty());
        if unverifiable {
            tracing::warn!(issue_id = %issue.id, "blocked marker of this run cannot be verified; flagging");
            let _ = refresher
                .add_labels(
                    &issue.id,
                    &[validation::LABEL_VALIDATION_MISSING.to_string()],
                )
                .await;
            return;
        }
        if !gate_enabled || layout == WorkspaceLayout::Legacy {
            return;
        }
        let started_dev = starts_development(&issue.state);
        let ended_review = state
            .as_deref()
            .is_some_and(|s| is_state(s, &["human review", "testing"]));
        if !(started_dev && ended_review) {
            return;
        }
        let ctx = self.checkout_context(issue);
        let outcome = post_run::gate_check(&workpad, repos, |dir, branch| {
            let ctx = ctx.clone();
            async move {
                crate::workspace::repos::refresh_single_root(&ctx, &dir, &branch)
                    .await
                    .is_ok()
            }
        })
        .await;
        if let post_run::GateOutcome::Failed(problems) = outcome {
            tracing::warn!(issue_id = %issue.id, problems = ?problems, "validation gate failed");
            let _ = refresher
                .add_labels(
                    &issue.id,
                    &[validation::LABEL_VALIDATION_MISSING.to_string()],
                )
                .await;
            if let Ok(Some((comment_id, body))) = refresher.fetch_workpad(&issue.id).await {
                let text = problems
                    .iter()
                    .map(|p| format!("- {p}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                if let Some(note) = post_run::with_section(&body, post_run::GATE_HEADING, &text) {
                    let _ = refresher.update_workpad(comment_id, &note).await;
                }
            }
        }
    }

    /// Compare the diff with `plan.md ## Files` when a development run
    /// handed off for review (P2 design §4 B2).
    #[allow(clippy::too_many_arguments)]
    async fn plan_deviation_check(
        &self,
        issue: &AgentIssue,
        layout: WorkspaceLayout,
        checked: &[post_run::CheckedRepo],
        repos: &[RepoContext],
        single: Option<&crate::config::SingleRepoSpec>,
        tracker_slug: &str,
        refresher: &Arc<dyn IssueStateRefresher>,
    ) -> Option<String> {
        if !refresher.supports_workpad() || !starts_development(&issue.state) {
            return None;
        }
        let ended_review = matches!(
            refresher.refresh_issue_state(&issue.id).await,
            Ok(Some(i)) if is_state(&i.state, &["human review", "testing"])
        );
        if !ended_review {
            return None;
        }
        let target = super::change_docs::doc_target(
            &issue.identifier,
            &issue.id,
            repos,
            single,
            tracker_slug,
        )?;
        let number = super::change_docs::issue_number(&issue.identifier, &issue.id)?;
        let multi_repo = layout == WorkspaceLayout::MultiRepo;
        let primary_alias = if multi_repo {
            repos
                .iter()
                .find(|r| r.role == "primary")
                .or_else(|| repos.first())
                .map(|r| r.alias.clone())
        } else {
            checked.first().map(|r| r.alias.clone())
        };
        let primary_alias = primary_alias?;
        let input = super::plan_check::PlanCheckInput {
            issue_id: &issue.id,
            issue_number: number,
            repos: checked,
            multi_repo,
            primary_alias: &primary_alias,
            primary_slug: &target.slug,
        };
        let result = super::plan_check::run(refresher.as_ref(), &input).await?;
        tracing::info!(
            issue_id = %issue.id,
            consistent = result.is_consistent(),
            unplanned = result.unplanned.len(),
            untouched = result.untouched.len(),
            "plan deviation check finished"
        );
        let prefix = if multi_repo {
            format!("{primary_alias}/")
        } else {
            String::new()
        };
        Some(super::plan_check::report(
            &result,
            &format!("{prefix}docs/changes/{number}/plan.md"),
        ))
    }

    /// Run the AI review when the issue was handed to Human Review (P2 §4 B1).
    #[allow(clippy::too_many_arguments)]
    async fn ai_review_check(
        &self,
        issue: &AgentIssue,
        layout: WorkspaceLayout,
        checked: &[post_run::CheckedRepo],
        repos: &[RepoContext],
        single: Option<&crate::config::SingleRepoSpec>,
        service: &crate::config::ServiceConfig,
        change_docs: &crate::prompt::ChangeDocsLookup,
        plan_report: Option<&str>,
        refresher: &Arc<dyn IssueStateRefresher>,
    ) {
        if !refresher.supports_workpad() || is_state(&issue.state, &["human review"]) {
            return;
        }
        let refreshed = match refresher.refresh_issue_state(&issue.id).await {
            Ok(Some(i)) if is_state(&i.state, &["human review"]) => i,
            _ => return,
        };
        let blocked_handoff = refreshed.labels.iter().any(|l| {
            l == validation::LABEL_VALIDATION_BLOCKED || l == validation::LABEL_BLOCKED_ACCESS
        });
        let Some(target) = super::change_docs::doc_target(
            &issue.identifier,
            &issue.id,
            repos,
            single,
            &service.tracker_project_slug,
        ) else {
            return;
        };
        let multi_repo = layout == WorkspaceLayout::MultiRepo;
        let review_repos: Vec<super::ai_review::ReviewRepo> = checked
            .iter()
            .map(|c| super::ai_review::ReviewRepo {
                alias: c.alias.clone(),
                dir: c.dir.clone(),
                default_branch: c.default_branch.clone(),
                slug: if multi_repo {
                    repos
                        .iter()
                        .find(|r| r.alias == c.alias)
                        .map(|r| r.slug.clone())
                        .unwrap_or_default()
                } else {
                    target.slug.clone()
                },
            })
            .collect();
        let primary_alias = if multi_repo {
            repos
                .iter()
                .find(|r| r.role == "primary")
                .or_else(|| repos.first())
                .map(|r| r.alias.clone())
        } else {
            checked.first().map(|c| c.alias.clone())
        };
        let Some(primary_alias) = primary_alias else {
            return;
        };
        let primary_dir = review_repos
            .iter()
            .find(|r| r.alias == primary_alias)
            .map(|r| r.dir.clone());
        let (docs, doc_refs): (Vec<std::path::PathBuf>, Vec<String>) =
            match (change_docs, primary_dir) {
                (crate::prompt::ChangeDocsLookup::Present(d), Some(dir)) => d
                    .files
                    .iter()
                    .map(|f| {
                        let short: String = f.blob_sha.chars().take(12).collect();
                        (
                            dir.join(&target.repo_dir).join(&f.name),
                            format!("{}@{short}", f.name),
                        )
                    })
                    .unzip(),
                _ => (Vec::new(), Vec::new()),
            };
        let input = super::ai_review::ReviewInput {
            issue_id: &issue.id,
            repos: &review_repos,
            primary_alias: &primary_alias,
            codex: &service.codex,
            timeout: std::time::Duration::from_millis(service.review.timeout_ms),
            docs: &docs,
            doc_refs: &doc_refs,
            plan_check: plan_report,
            // The issue is no longer active, so reconciliation may cancel the
            // worker's token; the review uses its own, bounded by `timeout`.
            cancel: CancellationToken::new(),
        };
        let Some(outcome) = super::ai_review::run(refresher.as_ref(), &input).await else {
            return;
        };
        let decision = super::ai_review::block_decision(&service.review, &outcome, blocked_handoff);
        tracing::info!(
            issue_id = %issue.id,
            round = outcome.round,
            important = ?outcome.important,
            nit = ?outcome.nit,
            ?decision,
            "AI review finished"
        );
        let unresolved = [super::ai_review::LABEL_UNRESOLVED.to_string()];
        match decision {
            super::ai_review::BlockDecision::SendBack => {
                let _ = refresher.remove_labels(&issue.id, &unresolved).await;
                if let Err(e) = refresher.set_issue_state(&issue.id, "In Progress").await {
                    tracing::warn!(issue_id = %issue.id, error = %e, "AI review: moving back to In Progress failed");
                }
            }
            super::ai_review::BlockDecision::Unresolved => {
                let _ = refresher.add_labels(&issue.id, &unresolved).await;
            }
            super::ai_review::BlockDecision::Clear => {
                let _ = refresher.remove_labels(&issue.id, &unresolved).await;
            }
            super::ai_review::BlockDecision::Keep => {}
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
        repo_instructions: Option<&str>,
        run_id: &str,
        change_docs: &crate::prompt::ChangeDocsLookup,
        layout: WorkspaceLayout,
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
            let mut prompt = self
                .prompt_engine
                .render_with_repos_ext(
                    &prompt_context,
                    attempt,
                    *turn_number,
                    max_turns,
                    repos,
                    &crate::prompt::RunGlobals {
                        run_id: run_id.to_string(),
                        workspace_layout: layout.as_str().to_string(),
                        change_docs: change_docs.clone(),
                    },
                )
                .map_err(|e| AgentError::PromptRender(e.to_string()))?;
            if *turn_number == 1 {
                if let Some(block) = repo_instructions {
                    prompt = format!("{block}\n{prompt}");
                }
            }

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

fn is_state(state: &str, names: &[&str]) -> bool {
    let n = state.trim().to_lowercase().replace(['_', '-'], " ");
    names.iter().any(|x| *x == n)
}

/// Runs starting in these states belong to development (not Merging).
fn starts_development(state: &str) -> bool {
    is_state(state, &["todo", "in progress", "rework"])
}

#[cfg(test)]
mod self_verification_tests {
    use super::*;

    #[test]
    fn state_helpers_normalize() {
        assert!(starts_development("In Progress"));
        assert!(starts_development("in_progress"));
        assert!(starts_development("Todo"));
        assert!(!starts_development("Merging"));
        assert!(is_state("Human Review", &["human review"]));
        assert!(is_state("human_review", &["human review"]));
    }
}

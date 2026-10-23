//! Multi-repository checkout (design: docs/方案/multi-repo-workspace-design.md §5).
//!
//! Only used when `workspace.repos` is non-empty. Each repository lives in
//! `<workspace>/<alias>/`. Clones happen in `<workspace>/.symphony/tmp/` and are
//! renamed into place only after they fully succeed, so a crash never leaves a
//! half-initialized repository at its final path.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::config::repos::{primary_key, CheckoutMode, RepoSpec};
use crate::proxy::proxy_command;

use super::WorkspaceError;

/// Per-repository state persisted in the workspace marker (schema v2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoState {
    pub slug: String,
    pub path: String,
    pub writable: bool,
    #[serde(default)]
    pub branch: Option<String>,
    pub base: String,
    #[serde(default)]
    pub head: Option<String>,
    pub status: RepoStatus,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepoStatus {
    Cloning,
    Ready,
    Failed,
}

/// Pending Rework branch switch, bound to the label event that triggered it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReworkPending {
    pub event_id: u64,
    pub suffix: u32,
}

/// Everything needed to talk to the git remote for one workspace.
#[derive(Debug, Clone)]
pub struct RepoCheckoutContext {
    /// Platform base, e.g. `http://gitlab.example.com:8081` (from `GITLAB_HOST`).
    pub platform_host: String,
    /// Token handed to git through `GIT_ASKPASS`; never written to disk.
    pub token: Option<String>,
    /// Directory outside every workspace that holds the askpass script.
    pub askpass_dir: PathBuf,
    /// Issue iid (or tracker id) used in branch names.
    pub issue_number: String,
    pub clone_timeout: Duration,
    pub fetch_timeout: Duration,
}

impl RepoCheckoutContext {
    pub fn clone_url(&self, slug: &str) -> String {
        format!("{}/{}.git", self.platform_host.trim_end_matches('/'), slug)
    }
}

/// Default working branch for writable repos (D4):
/// `symphony/<primary-key>-<iid>` or `...-r<n>` for Rework round `n >= 2`.
pub fn work_branch(
    primary_repo_name: &str,
    issue_number: &str,
    rework_suffix: Option<u32>,
) -> String {
    let base = format!(
        "symphony/{}-{}",
        primary_key(primary_repo_name),
        issue_number
    );
    match rework_suffix {
        Some(n) => format!("{base}-r{n}"),
        None => base,
    }
}

/// Exact branch whitelist used by the pre-push hook and integrity checks.
pub fn branch_matches(branch: &str, primary_repo_name: &str, issue_number: &str) -> bool {
    let base = format!(
        "symphony/{}-{}",
        primary_key(primary_repo_name),
        issue_number
    );
    match branch.strip_prefix(&base) {
        Some("") => true,
        Some(rest) => rest
            .strip_prefix("-r")
            .map(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or(false),
        None => false,
    }
}

/// Highest `-r<n>` suffix among branch names for this issue (1 = base branch).
pub fn max_suffix<'a>(
    branches: impl IntoIterator<Item = &'a str>,
    primary_repo_name: &str,
    issue_number: &str,
) -> u32 {
    let base = format!(
        "symphony/{}-{}",
        primary_key(primary_repo_name),
        issue_number
    );
    branches
        .into_iter()
        .filter_map(|b| {
            let b = b.strip_prefix("refs/heads/").unwrap_or(b);
            match b.strip_prefix(&base)? {
                "" => Some(1),
                rest => rest.strip_prefix("-r")?.parse::<u32>().ok(),
            }
        })
        .max()
        .unwrap_or(0)
}

/// Compare two remote URLs as "same repository": userinfo, `.git`, trailing
/// slash, scheme and case are ignored (design §3.1).
pub fn same_remote(a: &str, b: &str) -> bool {
    match (normalize_remote(a), normalize_remote(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

fn normalize_remote(url: &str) -> Option<String> {
    let url = url.trim();
    let rest = if let Some(r) = url.strip_prefix("git@") {
        let (host, path) = r.split_once(':')?;
        format!("{host}/{path}")
    } else {
        let (_, r) = url.split_once("://")?;
        let (authority, path) = r.split_once('/')?;
        let host = authority.rsplit('@').next().unwrap_or(authority);
        format!("{host}/{path}")
    };
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    Some(rest.to_lowercase())
}

/// Strip credentials from a URL for logging / error messages.
fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for token in text.split_inclusive(char::is_whitespace) {
        if let Some((scheme, rest)) = token.split_once("://") {
            if let Some((_, after)) = rest.split_once('@') {
                out.push_str(scheme);
                out.push_str("://***@");
                out.push_str(after);
                continue;
            }
        }
        out.push_str(token);
    }
    out
}

/// Kills the whole git process group when dropped (worker abort, timeout).
/// `kill_on_drop` alone would only reach the direct child, not
/// `git-remote-http` and friends.
struct ProcessGroupGuard {
    pgid: Option<i32>,
}

impl ProcessGroupGuard {
    fn disarm(&mut self) {
        self.pgid = None;
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pgid) = self.pgid {
            // SAFETY: killpg with a pgid we created via process_group(0).
            unsafe {
                libc::killpg(pgid, libc::SIGKILL);
            }
        }
    }
}

/// Write the askpass helper. It reads the token from an environment variable
/// set only on the git child, so the script file never contains a secret.
async fn ensure_askpass(dir: &Path) -> Result<PathBuf, WorkspaceError> {
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| WorkspaceError::Io { source: e })?;
    let path = dir.join("git-askpass.sh");
    let script = "#!/bin/sh\ncase \"$1\" in\n  Username*) printf '%s\\n' oauth2 ;;\n  *) printf '%s\\n' \"$SYMPHONY_GIT_TOKEN\" ;;\nesac\n";
    let needs_write = match tokio::fs::read_to_string(&path).await {
        Ok(existing) => existing != script,
        Err(_) => true,
    };
    if needs_write {
        tokio::fs::write(&path, script)
            .await
            .map_err(|e| WorkspaceError::Io { source: e })?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .await
            .map_err(|e| WorkspaceError::Io { source: e })?;
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .await
            .map_err(|e| WorkspaceError::Io { source: e })?;
    }
    Ok(path)
}

/// Run one git command with a hard timeout and process-group cleanup.
async fn git(
    ctx: &RepoCheckoutContext,
    askpass: Option<&Path>,
    cwd: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<String, WorkspaceError> {
    git_with_progress(ctx, askpass, cwd, args, timeout, &|| {}).await
}

/// Like [`git`], calling `on_progress` every 30s while waiting so a long
/// clone keeps the worker's stall timer fresh (design §5).
async fn git_with_progress(
    ctx: &RepoCheckoutContext,
    askpass: Option<&Path>,
    cwd: &Path,
    args: &[&str],
    timeout: Duration,
    on_progress: &(dyn Fn() + Send + Sync),
) -> Result<String, WorkspaceError> {
    let mut cmd: Command = proxy_command("git");
    cmd.args(["-c", "credential.helper="])
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let (Some(askpass), Some(token)) = (askpass, ctx.token.as_deref()) {
        cmd.env("GIT_ASKPASS", askpass)
            .env("SYMPHONY_GIT_TOKEN", token);
    }
    #[cfg(unix)]
    cmd.process_group(0);

    let child = cmd
        .spawn()
        .map_err(|e| WorkspaceError::HookIo { source: e })?;
    let mut guard = ProcessGroupGuard {
        pgid: child.id().map(|p| p as i32),
    };
    let wait = child.wait_with_output();
    tokio::pin!(wait);
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    let mut ticker = tokio::time::interval(Duration::from_secs(30));
    ticker.tick().await; // first tick is immediate
    let outcome = loop {
        tokio::select! {
            result = &mut wait => break Some(result),
            _ = &mut deadline => break None,
            _ = ticker.tick() => on_progress(),
        }
    };
    let output = match outcome {
        Some(result) => result.map_err(|e| WorkspaceError::HookIo { source: e })?,
        None => {
            return Err(WorkspaceError::RepoCheckout {
                reason: format!(
                    "git {} timed out after {}s",
                    args.first().unwrap_or(&""),
                    timeout.as_secs()
                ),
            });
        }
    };
    guard.disarm();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(WorkspaceError::RepoCheckout {
            reason: format!(
                "git {} failed ({}): {}",
                args.first().unwrap_or(&""),
                output.status,
                redact(stderr.trim()).chars().take(500).collect::<String>()
            ),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Clone / refresh / configure all repositories of one workspace.
pub struct RepoCheckout<'a> {
    pub ctx: &'a RepoCheckoutContext,
    pub workspace: &'a Path,
    pub repos: &'a [RepoSpec],
}

impl RepoCheckout<'_> {
    fn primary(&self) -> &RepoSpec {
        self.repos
            .iter()
            .find(|r| r.role == crate::config::repos::RepoRole::Primary)
            .unwrap_or(&self.repos[0])
    }

    fn tmp_dir(&self) -> PathBuf {
        self.workspace.join(".symphony").join("tmp")
    }

    /// Drop leftovers of interrupted clones. Safe: tmp never holds user work.
    pub async fn clear_tmp(&self) -> Result<(), WorkspaceError> {
        let tmp = self.tmp_dir();
        if tmp.exists() {
            make_tree_writable(&tmp).await;
            tokio::fs::remove_dir_all(&tmp)
                .await
                .map_err(|e| WorkspaceError::Io { source: e })?;
        }
        Ok(())
    }

    /// Bring every repository to a usable state, returning the new per-repo
    /// state map. `previous` is the state from the marker (may be empty).
    ///
    /// `branch_override` maps alias -> branch for a pending Rework switch.
    pub async fn sync(
        &self,
        previous: &BTreeMap<String, RepoState>,
        on_progress: &(dyn Fn() + Send + Sync),
    ) -> Result<BTreeMap<String, RepoState>, (BTreeMap<String, RepoState>, WorkspaceError)> {
        let mut states = previous.clone();
        let askpass = match ensure_askpass(&self.ctx.askpass_dir).await {
            Ok(p) => p,
            Err(e) => return Err((states, e)),
        };
        let primary_name = self.primary().repo_name().to_string();

        for repo in self.repos {
            on_progress();
            let result = self
                .sync_one(
                    repo,
                    states.get(&repo.alias),
                    &askpass,
                    &primary_name,
                    on_progress,
                )
                .await;
            match result {
                Ok(state) => {
                    states.insert(repo.alias.clone(), state);
                }
                Err(e) => {
                    let mut failed = states.remove(&repo.alias).unwrap_or_else(|| RepoState {
                        slug: repo.slug.clone(),
                        path: repo.alias.clone(),
                        writable: repo.writable(),
                        branch: None,
                        base: repo.default_branch.clone(),
                        head: None,
                        status: RepoStatus::Failed,
                        last_error: None,
                    });
                    // A ready repo whose refresh failed stays ready (§5.1):
                    // only first-time clones become `failed`.
                    if failed.status != RepoStatus::Ready {
                        failed.status = RepoStatus::Failed;
                    }
                    failed.last_error = Some(e.to_string());
                    states.insert(repo.alias.clone(), failed);
                    return Err((states, e));
                }
            }
        }
        // Successful clones were renamed out; drop the (now empty) tmp dir.
        let _ = tokio::fs::remove_dir(self.tmp_dir()).await;
        Ok(states)
    }

    async fn sync_one(
        &self,
        repo: &RepoSpec,
        previous: Option<&RepoState>,
        askpass: &Path,
        primary_name: &str,
        on_progress: &(dyn Fn() + Send + Sync),
    ) -> Result<RepoState, WorkspaceError> {
        let dest = self.workspace.join(&repo.alias);
        let url = self.ctx.clone_url(&repo.slug);
        let desired_branch = repo.writable().then(|| {
            previous
                .and_then(|p| p.branch.clone())
                .unwrap_or_else(|| work_branch(primary_name, &self.ctx.issue_number, None))
        });

        match previous {
            Some(prev) if prev.status == RepoStatus::Ready && dest.join(".git").exists() => {
                // Refresh: fetch only, never switch or reset local work.
                if let Err(e) = self
                    .refresh(repo, &dest, desired_branch.as_deref(), askpass)
                    .await
                {
                    tracing::warn!(alias = %repo.alias, error = %e, "repo refresh failed; continuing with local state");
                    let mut kept = prev.clone();
                    kept.last_error = Some(e.to_string());
                    return Ok(kept);
                }
                let mut kept = prev.clone();
                kept.last_error = None;
                // Re-apply guards idempotently (hooks may have been tampered with).
                self.configure(repo, &dest, desired_branch.as_deref(), primary_name, false)
                    .await?;
                kept.head = Some(rev_parse(self.ctx, &dest, "HEAD").await?);
                Ok(kept)
            }
            _ if dest.exists() => {
                // Directory exists but the marker does not say ready: the
                // rename may have completed before the marker write (§5.1).
                let origin = git(
                    self.ctx,
                    None,
                    &dest,
                    &["remote", "get-url", "origin"],
                    self.ctx.fetch_timeout,
                )
                .await
                .ok();
                let head_ok = rev_parse(self.ctx, &dest, "HEAD").await.is_ok();
                if !(dest.join(".git").exists()
                    && head_ok
                    && origin.as_deref().is_some_and(|o| same_remote(o, &url)))
                {
                    return Err(WorkspaceError::ManualRecoveryRequired {
                        reason: format!("repo_dir_unexpected:{}", repo.alias),
                    });
                }
                self.configure(repo, &dest, desired_branch.as_deref(), primary_name, true)
                    .await?;
                Ok(self.ready_state(repo, &dest, desired_branch).await?)
            }
            _ => {
                on_progress();
                let tmp_root = self.tmp_dir();
                tokio::fs::create_dir_all(&tmp_root)
                    .await
                    .map_err(|e| WorkspaceError::Io { source: e })?;
                let tmp = tmp_root.join(format!(
                    "{}-{}",
                    repo.alias,
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
                ));
                let tmp_str = tmp.to_string_lossy().to_string();
                let mut args = vec!["clone", "--no-checkout", "--origin", "origin"];
                if repo.checkout == CheckoutMode::Partial {
                    args.push("--filter=blob:none");
                }
                args.push(&url);
                args.push(&tmp_str);
                git_with_progress(
                    self.ctx,
                    Some(askpass),
                    self.workspace,
                    &args,
                    self.ctx.clone_timeout,
                    on_progress,
                )
                .await?;
                on_progress();
                tokio::fs::rename(&tmp, &dest)
                    .await
                    .map_err(|e| WorkspaceError::Io { source: e })?;
                self.refresh(repo, &dest, desired_branch.as_deref(), askpass)
                    .await?;
                self.configure(repo, &dest, desired_branch.as_deref(), primary_name, true)
                    .await?;
                Ok(self.ready_state(repo, &dest, desired_branch).await?)
            }
        }
    }

    async fn ready_state(
        &self,
        repo: &RepoSpec,
        dest: &Path,
        branch: Option<String>,
    ) -> Result<RepoState, WorkspaceError> {
        Ok(RepoState {
            slug: repo.slug.clone(),
            path: repo.alias.clone(),
            writable: repo.writable(),
            branch,
            base: repo.default_branch.clone(),
            head: Some(rev_parse(self.ctx, dest, "HEAD").await?),
            status: RepoStatus::Ready,
            last_error: None,
        })
    }

    /// `fetch origin <default_branch>` plus the work branch if it exists remotely.
    async fn refresh(
        &self,
        repo: &RepoSpec,
        dest: &Path,
        work_branch: Option<&str>,
        askpass: &Path,
    ) -> Result<(), WorkspaceError> {
        let base_spec = format!(
            "+refs/heads/{0}:refs/remotes/origin/{0}",
            repo.default_branch
        );
        let mut specs = vec![base_spec];
        if let Some(branch) = work_branch {
            let remote = git(
                self.ctx,
                Some(askpass),
                dest,
                &["ls-remote", "--heads", "origin", branch],
                self.ctx.fetch_timeout,
            )
            .await?;
            if !remote.is_empty() {
                specs.push(format!("+refs/heads/{branch}:refs/remotes/origin/{branch}"));
            }
        }
        let mut args = vec!["fetch", "--no-tags", "origin"];
        args.extend(specs.iter().map(String::as_str));
        git(self.ctx, Some(askpass), dest, &args, self.ctx.fetch_timeout).await?;
        Ok(())
    }

    /// Check out the right ref and (re)install push guards. `initial` controls
    /// whether a checkout happens (never on refresh, to keep local work).
    async fn configure(
        &self,
        repo: &RepoSpec,
        dest: &Path,
        work_branch: Option<&str>,
        primary_name: &str,
        initial: bool,
    ) -> Result<(), WorkspaceError> {
        let t = self.ctx.fetch_timeout;
        make_tree_writable(dest).await;
        if initial {
            match work_branch {
                Some(branch) => self.checkout_work_branch(repo, dest, branch).await?,
                None => {
                    let target = format!("origin/{}", repo.default_branch);
                    git(
                        self.ctx,
                        None,
                        dest,
                        &["checkout", "--detach", "--force", &target],
                        t,
                    )
                    .await?;
                }
            }
        }
        // Never let a token-bearing URL persist in .git/config.
        let fetch_url = self.ctx.clone_url(&repo.slug);
        git(
            self.ctx,
            None,
            dest,
            &["remote", "set-url", "origin", &fetch_url],
            t,
        )
        .await?;
        git(
            self.ctx,
            None,
            dest,
            &["config", "credential.helper", ""],
            t,
        )
        .await?;
        let hooks_dir = dest.join(".git").join("hooks");
        tokio::fs::create_dir_all(&hooks_dir)
            .await
            .map_err(|e| WorkspaceError::Io { source: e })?;
        let pre_push = if repo.writable() {
            let base = format!(
                "symphony/{}-{}",
                primary_key(primary_name),
                self.ctx.issue_number
            );
            git(
                self.ctx,
                None,
                dest,
                &["remote", "set-url", "--push", "origin", &fetch_url],
                t,
            )
            .await?;
            pre_push_whitelist(&base)
        } else {
            git(
                self.ctx,
                None,
                dest,
                &[
                    "remote",
                    "set-url",
                    "--push",
                    "origin",
                    "no-push://disabled",
                ],
                t,
            )
            .await?;
            "#!/bin/sh\necho 'symphony: this repository is read-only in this workspace; push refused.' >&2\nexit 1\n".to_string()
        };
        let hook_path = hooks_dir.join("pre-push");
        tokio::fs::write(&hook_path, pre_push)
            .await
            .map_err(|e| WorkspaceError::Io { source: e })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&hook_path, std::fs::Permissions::from_mode(0o755))
                .await
                .map_err(|e| WorkspaceError::Io { source: e })?;
        }
        if !repo.writable() {
            make_files_readonly(dest).await;
        }
        Ok(())
    }

    async fn checkout_work_branch(
        &self,
        repo: &RepoSpec,
        dest: &Path,
        branch: &str,
    ) -> Result<(), WorkspaceError> {
        let t = self.ctx.fetch_timeout;
        let local = git(
            self.ctx,
            None,
            dest,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ],
            t,
        )
        .await
        .is_ok();
        if local {
            git(self.ctx, None, dest, &["checkout", branch], t).await?;
            return Ok(());
        }
        let remote_ref = format!("refs/remotes/origin/{branch}");
        let start = if git(
            self.ctx,
            None,
            dest,
            &["rev-parse", "--verify", "--quiet", &remote_ref],
            t,
        )
        .await
        .is_ok()
        {
            format!("origin/{branch}")
        } else {
            format!("origin/{}", repo.default_branch)
        };
        git(self.ctx, None, dest, &["checkout", "-b", branch, &start], t).await?;
        Ok(())
    }

    /// Switch every writable repo to the Rework branch `-r<suffix>` (§5.1).
    /// Idempotent: an existing local branch is just checked out.
    pub async fn switch_branches(
        &self,
        states: &mut BTreeMap<String, RepoState>,
        suffix: u32,
    ) -> Result<(), WorkspaceError> {
        let primary_name = self.primary().repo_name().to_string();
        let branch = work_branch(&primary_name, &self.ctx.issue_number, Some(suffix));
        let t = self.ctx.fetch_timeout;
        for repo in self.repos.iter().filter(|r| r.writable()) {
            let dest = self.workspace.join(&repo.alias);
            // Rework restarts from scratch, but never destroys work: leftover
            // uncommitted changes from the previous round are stashed (and
            // stay recoverable via `git stash list`) instead of blocking the
            // checkout forever.
            let dirty = git(self.ctx, None, &dest, &["status", "--porcelain"], t).await?;
            if !dirty.is_empty() {
                let msg = format!("symphony: WIP before {branch}");
                git(
                    self.ctx,
                    None,
                    &dest,
                    &[
                        "-c",
                        "user.name=symphony",
                        "-c",
                        "user.email=symphony@localhost",
                        "stash",
                        "push",
                        "--include-untracked",
                        "-m",
                        &msg,
                    ],
                    t,
                )
                .await?;
                tracing::info!(alias = %repo.alias, branch = %branch, "stashed uncommitted changes before Rework switch");
            }
            self.checkout_work_branch(repo, &dest, &branch).await?;
            if let Some(state) = states.get_mut(&repo.alias) {
                state.branch = Some(branch.clone());
                state.head = Some(rev_parse(self.ctx, &dest, "HEAD").await?);
            }
        }
        Ok(())
    }

    /// Highest rework suffix known locally (marker branches) or remotely.
    pub async fn highest_suffix(&self, states: &BTreeMap<String, RepoState>) -> u32 {
        let primary_name = self.primary().repo_name().to_string();
        let mut names: Vec<String> = states.values().filter_map(|s| s.branch.clone()).collect();
        let pattern = format!(
            "refs/heads/symphony/{}-{}*",
            primary_key(&primary_name),
            self.ctx.issue_number
        );
        if let Ok(askpass) = ensure_askpass(&self.ctx.askpass_dir).await {
            for repo in self.repos.iter().filter(|r| r.writable()) {
                let dest = self.workspace.join(&repo.alias);
                if let Ok(out) = git(
                    self.ctx,
                    Some(&askpass),
                    &dest,
                    &["ls-remote", "--heads", "origin", &pattern],
                    self.ctx.fetch_timeout,
                )
                .await
                {
                    names.extend(
                        out.lines()
                            .filter_map(|l| l.split_whitespace().nth(1))
                            .map(str::to_string),
                    );
                }
            }
        }
        max_suffix(
            names.iter().map(String::as_str),
            &primary_name,
            &self.ctx.issue_number,
        )
    }

    /// Read-only integrity check run after the agent (§5.3 items 1).
    /// Returns human-readable violations; empty = clean.
    pub async fn readonly_violations(&self, states: &BTreeMap<String, RepoState>) -> Vec<String> {
        let mut out = Vec::new();
        for repo in self.repos.iter().filter(|r| !r.writable()) {
            let dest = self.workspace.join(&repo.alias);
            let Some(state) = states.get(&repo.alias) else {
                continue;
            };
            let head = rev_parse(self.ctx, &dest, "HEAD").await.ok();
            if head.is_some() && head != state.head {
                out.push(format!(
                    "{}: HEAD moved from {:?} to {:?}",
                    repo.alias, state.head, head
                ));
            }
            match git(
                self.ctx,
                None,
                &dest,
                &["status", "--porcelain"],
                self.ctx.fetch_timeout,
            )
            .await
            {
                Ok(s) if !s.is_empty() => {
                    out.push(format!("{}: working tree modified", repo.alias))
                }
                Err(e) => out.push(format!("{}: status check failed: {e}", repo.alias)),
                _ => {}
            }
        }
        out
    }
}

fn pre_push_whitelist(base: &str) -> String {
    format!(
        "#!/bin/sh\n# Installed by Symphony: only the issue work branch may be pushed.\n\
         while read local_ref local_sha remote_ref remote_sha; do\n\
         \x20 branch=\"${{remote_ref#refs/heads/}}\"\n\
         \x20 case \"$branch\" in\n\
         \x20   '{base}') ;;\n\
         \x20   '{base}'-r*) suffix=\"${{branch#'{base}'-r}}\"; case \"$suffix\" in ''|*[!0-9]*) echo \"symphony: refusing to push $branch; use {base}\" >&2; exit 1;; esac ;;\n\
         \x20   *) echo \"symphony: refusing to push $branch; only {base} (or {base}-r<n>) is allowed\" >&2; exit 1 ;;\n\
         \x20 esac\n\
         done\n\
         exit 0\n"
    )
}

async fn rev_parse(
    ctx: &RepoCheckoutContext,
    dest: &Path,
    rev: &str,
) -> Result<String, WorkspaceError> {
    git(ctx, None, dest, &["rev-parse", rev], ctx.fetch_timeout).await
}

/// Remove write permission from tracked working-tree files (not directories,
/// so `remove_dir_all` keeps working; not `.git`, so fetch keeps working).
async fn make_files_readonly(dest: &Path) {
    set_tree_file_mode(dest.to_path_buf(), false).await;
}

/// Restore write permission on every file below `path` (before GC/quarantine
/// or re-configuration).
pub async fn make_tree_writable(path: &Path) {
    set_tree_file_mode(path.to_path_buf(), true).await;
}

async fn set_tree_file_mode(root: PathBuf, writable: bool) {
    let _ = tokio::task::spawn_blocking(move || {
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(ft) = entry.file_type() else { continue };
                if ft.is_symlink() {
                    continue;
                }
                if ft.is_dir() {
                    if !writable && path.file_name().is_some_and(|n| n == ".git") {
                        continue;
                    }
                    stack.push(path);
                } else if let Ok(meta) = entry.metadata() {
                    let mut perm = meta.permissions();
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let mode = perm.mode();
                        let new = if writable {
                            mode | 0o200
                        } else {
                            mode & !0o222
                        };
                        if new != mode {
                            perm.set_mode(new);
                            let _ = std::fs::set_permissions(&path, perm);
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        perm.set_readonly(!writable);
                        let _ = std::fs::set_permissions(&path, perm);
                    }
                }
            }
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_names_follow_d4() {
        assert_eq!(
            work_branch("jst-ai-studio", "32", None),
            "symphony/jst-ai-studio-32"
        );
        assert_eq!(
            work_branch("Jst_AI", "32", Some(3)),
            "symphony/jst-ai-32-r3"
        );
    }

    #[test]
    fn branch_whitelist_is_exact() {
        assert!(branch_matches(
            "symphony/jst-ai-studio-32",
            "jst-ai-studio",
            "32"
        ));
        assert!(branch_matches(
            "symphony/jst-ai-studio-32-r2",
            "jst-ai-studio",
            "32"
        ));
        assert!(!branch_matches(
            "symphony/jst-ai-studio-320",
            "jst-ai-studio",
            "32"
        ));
        assert!(!branch_matches(
            "symphony/jst-ai-studio-32-r",
            "jst-ai-studio",
            "32"
        ));
        assert!(!branch_matches(
            "symphony/jst-ai-studio-32-rx",
            "jst-ai-studio",
            "32"
        ));
        assert!(!branch_matches("dev", "jst-ai-studio", "32"));
    }

    #[test]
    fn max_suffix_counts_base_as_one() {
        let names = [
            "refs/heads/symphony/x-32",
            "symphony/x-32-r4",
            "symphony/x-320-r9",
            "symphony/x-32-rz",
        ];
        assert_eq!(max_suffix(names, "x", "32"), 4);
        assert_eq!(max_suffix(["symphony/x-32"], "x", "32"), 1);
        assert_eq!(max_suffix(Vec::<&str>::new(), "x", "32"), 0);
    }

    #[test]
    fn same_remote_ignores_credentials_suffix_and_case() {
        let a =
            "http://oauth2:glpat-secret@gitlab.jushuitan-inc.com:8081/zimei10525/jst-ai-studio.git";
        let b = "http://gitlab.jushuitan-inc.com:8081/Zimei10525/jst-ai-studio/";
        assert!(same_remote(a, b));
        assert!(same_remote("git@h:g/r.git", "https://h/g/r"));
        assert!(!same_remote(
            a,
            "http://gitlab.jushuitan-inc.com:8081/JstAi/jxy.git"
        ));
        assert!(!same_remote("not a url", b));
    }

    #[test]
    fn redact_hides_userinfo() {
        let s = redact("fatal: unable to access 'http://oauth2:glpat-x@h/a.git/': 403");
        assert!(!s.contains("glpat-x"), "{s}");
        assert!(s.contains("***@h/a.git"));
    }

    #[test]
    fn pre_push_hook_script_mentions_branch() {
        let s = pre_push_whitelist("symphony/x-32");
        assert!(s.starts_with("#!/bin/sh"));
        assert!(s.contains("'symphony/x-32')"));
    }
}

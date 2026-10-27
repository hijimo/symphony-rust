//! WorkspaceService (workbench design §4.1): one git worktree per issue,
//! created from a per-repository bare mirror. Layout matches the
//! implementation-phase workspace: single-repo checkouts live at the root,
//! multi-repo workspaces have `<alias>/` per repository.

use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use tokio::process::Command;

use super::content::content_version;
use crate::error::WebPlatformError;

const GIT_TIMEOUT: Duration = Duration::from_secs(600);
/// Files larger than this are returned as metadata only (workbench §5).
pub const MAX_TEXT_FILE: u64 = 1024 * 1024;
const MAX_SEARCH_RESULTS: usize = 200;

/// One repository of a workspace.
#[derive(Debug, Clone)]
pub struct RepoCheckout {
    /// `None` for the single-repo root checkout.
    pub alias: Option<String>,
    /// Clone URL (credentials are passed through `GIT_ASKPASS`, never stored).
    pub clone_url: String,
    pub default_branch: String,
    pub writable: bool,
}

/// Workspace description built from a project.
#[derive(Debug, Clone)]
pub struct WorkspaceSpec {
    pub project_id: i64,
    pub key: String,
    pub primary: RepoCheckout,
    pub secondaries: Vec<RepoCheckout>,
    /// Branch the primary repo works on.
    pub branch: String,
    pub token: Option<String>,
}

impl WorkspaceSpec {
    pub fn is_multi_repo(&self) -> bool {
        !self.secondaries.is_empty()
    }

    /// Primary repo directory relative to the workspace root ("" or "<alias>").
    pub fn primary_rel(&self) -> String {
        self.primary.alias.clone().unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TreeEntry {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub size: Option<u64>,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileContent {
    pub path: String,
    pub content: Option<String>,
    pub version: String,
    pub size: u64,
    pub binary: bool,
    pub too_large: bool,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub path: String,
    pub line: Option<u64>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContextStatus {
    pub base_sha: Option<String>,
    pub behind_main: bool,
    pub missing: Vec<String>,
}

/// Root of all editor workspaces: `<workspace_root>/editor`.
#[derive(Debug, Clone)]
pub struct WorkspaceService {
    root: PathBuf,
}

fn git_err(msg: impl Into<String>) -> WebPlatformError {
    WebPlatformError::ExternalService(msg.into())
}

fn sanitize_key(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

impl WorkspaceService {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            root: workspace_root.into().join("editor"),
        }
    }

    pub fn workspace_dir(&self, project_id: i64, key: &str) -> PathBuf {
        self.root
            .join(project_id.to_string())
            .join(sanitize_key(key))
    }

    fn mirror_dir(&self, project_id: i64, alias: Option<&str>) -> PathBuf {
        self.root
            .join("mirrors")
            .join(project_id.to_string())
            .join(format!("{}.git", sanitize_key(alias.unwrap_or("primary"))))
    }

    fn askpass_dir(&self) -> PathBuf {
        self.root.join(".askpass")
    }

    pub fn repo_dir(&self, spec: &WorkspaceSpec, alias: Option<&str>) -> PathBuf {
        let ws = self.workspace_dir(spec.project_id, &spec.key);
        match alias {
            Some(a) => ws.join(a),
            None => ws,
        }
    }

    pub fn primary_dir(&self, spec: &WorkspaceSpec) -> PathBuf {
        self.repo_dir(spec, spec.primary.alias.as_deref())
    }

    async fn ensure_askpass(&self) -> Result<PathBuf, WebPlatformError> {
        let dir = self.askpass_dir();
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| WebPlatformError::Internal(e.to_string()))?;
        let path = dir.join("askpass.sh");
        if !path.exists() {
            tokio::fs::write(
                &path,
                "#!/bin/sh\ncase \"$1\" in\n  Username*) echo \"${SYMPHONY_GIT_USER:-oauth2}\" ;;\n  *) echo \"$SYMPHONY_GIT_TOKEN\" ;;\nesac\n",
            )
            .await
            .map_err(|e| WebPlatformError::Internal(e.to_string()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700));
            }
        }
        Ok(path)
    }

    /// Run git; credentials go through GIT_ASKPASS env only.
    pub async fn git(
        &self,
        cwd: &Path,
        args: &[&str],
        token: Option<&str>,
    ) -> Result<String, WebPlatformError> {
        let mut cmd = Command::new("git");
        // Bare mirrors: pass --git-dir explicitly so `safe.bareRepository=explicit`
        // setups keep working.
        if cwd.extension().is_some_and(|e| e == "git") && cwd.join("HEAD").exists() {
            cmd.arg("--git-dir").arg(cwd);
        }
        cmd.current_dir(cwd)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(t) = token {
            let askpass = self.ensure_askpass().await?;
            cmd.env("GIT_ASKPASS", askpass).env("SYMPHONY_GIT_TOKEN", t);
        }
        let out = tokio::time::timeout(GIT_TIMEOUT, cmd.output())
            .await
            .map_err(|_| git_err(format!("git {} timed out", args.join(" "))))?
            .map_err(|e| git_err(format!("git failed to start: {e}")))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let redacted = match token {
                Some(t) if !t.is_empty() => stderr.replace(t, "***"),
                _ => stderr.to_string(),
            };
            return Err(git_err(format!(
                "git {} failed: {}",
                args.first().copied().unwrap_or(""),
                redacted.trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    async fn ensure_mirror(
        &self,
        project_id: i64,
        repo: &RepoCheckout,
        token: Option<&str>,
    ) -> Result<PathBuf, WebPlatformError> {
        let mirror = self.mirror_dir(project_id, repo.alias.as_deref());
        if mirror.join("HEAD").exists() {
            self.git(&mirror, &["fetch", "--prune", "origin"], token)
                .await?;
        } else {
            let parent = mirror.parent().unwrap().to_path_buf();
            tokio::fs::create_dir_all(&parent)
                .await
                .map_err(|e| WebPlatformError::Internal(e.to_string()))?;
            let name = mirror.file_name().unwrap().to_string_lossy().to_string();
            self.git(&parent, &["clone", "--bare", &repo.clone_url, &name], token)
                .await?;
            // A bare clone does not configure a fetch refspec for remote branches.
            self.git(
                &mirror,
                &[
                    "config",
                    "remote.origin.fetch",
                    "+refs/heads/*:refs/remotes/origin/*",
                ],
                None,
            )
            .await?;
            self.git(&mirror, &["fetch", "--prune", "origin"], token)
                .await?;
        }
        Ok(mirror)
    }

    async fn remote_branch_exists(&self, mirror: &Path, branch: &str) -> bool {
        self.git(
            mirror,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/remotes/origin/{branch}"),
            ],
            None,
        )
        .await
        .is_ok()
    }

    /// Create or refresh the workspace (§4.1 初始化与同步). Idempotent.
    pub async fn ensure(&self, spec: &WorkspaceSpec) -> Result<ContextStatus, WebPlatformError> {
        let token = spec.token.as_deref();
        let ws = self.workspace_dir(spec.project_id, &spec.key);
        if spec.is_multi_repo() {
            tokio::fs::create_dir_all(&ws)
                .await
                .map_err(|e| WebPlatformError::Internal(e.to_string()))?;
        }
        let mut status = ContextStatus::default();

        // Primary repository: work branch.
        let mirror = self
            .ensure_mirror(spec.project_id, &spec.primary, token)
            .await?;
        let dir = self.primary_dir(spec);
        if !dir.join(".git").exists() {
            if let Some(parent) = dir.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| WebPlatformError::Internal(e.to_string()))?;
            }
            let dir_s = dir.to_string_lossy().to_string();
            let base = format!("origin/{}", spec.primary.default_branch);
            let start = if self.remote_branch_exists(&mirror, &spec.branch).await {
                format!("origin/{}", spec.branch)
            } else {
                base.clone()
            };
            // Local branch may linger from a removed worktree.
            let _ = self.git(&mirror, &["worktree", "prune"], None).await;
            self.git(
                &mirror,
                &["worktree", "add", "-B", &spec.branch, &dir_s, &start],
                None,
            )
            .await?;
            self.git(&dir, &["config", "user.name", "Symphony"], None)
                .await?;
            self.git(&dir, &["config", "user.email", "symphony@localhost"], None)
                .await?;
            self.git(
                &dir,
                &[
                    "remote",
                    "set-url",
                    "--push",
                    "origin",
                    &spec.primary.clone_url,
                ],
                None,
            )
            .await
            .ok();
        }
        // Submodules / LFS (best effort; failures reported, not fatal).
        if dir.join(".gitmodules").exists() {
            if let Err(e) = self
                .git(
                    &dir,
                    &["submodule", "update", "--init", "--recursive"],
                    token,
                )
                .await
            {
                status.missing.push(format!("submodules: {e}"));
            }
        }
        if let Ok(attrs) = tokio::fs::read_to_string(dir.join(".gitattributes")).await {
            if attrs.contains("filter=lfs") {
                if let Err(e) = self.git(&dir, &["lfs", "pull"], token).await {
                    status.missing.push(format!("lfs: {e}"));
                }
            }
        }

        // Secondary repositories: read-only, detached at default branch head.
        for repo in &spec.secondaries {
            let alias = repo.alias.as_deref().unwrap_or("repo");
            let m = match self.ensure_mirror(spec.project_id, repo, token).await {
                Ok(m) => m,
                Err(e) => {
                    status.missing.push(format!("{alias}: {e}"));
                    continue;
                }
            };
            let d = ws.join(alias);
            let target = format!("origin/{}", repo.default_branch);
            if d.join(".git").exists() {
                if let Err(e) = self.git(&d, &["checkout", "--detach", &target], None).await {
                    status.missing.push(format!("{alias}: {e}"));
                }
            } else {
                let _ = self.git(&m, &["worktree", "prune"], None).await;
                let d_s = d.to_string_lossy().to_string();
                if let Err(e) = self
                    .git(&m, &["worktree", "add", "--detach", &d_s, &target], None)
                    .await
                {
                    status.missing.push(format!("{alias}: {e}"));
                }
            }
        }

        let base = format!("origin/{}", spec.primary.default_branch);
        status.base_sha = self
            .git(&dir, &["rev-parse", &base], None)
            .await
            .ok()
            .map(|s| s.trim().to_string());
        status.behind_main = self
            .git(
                &dir,
                &["rev-list", "--count", &format!("HEAD..{base}")],
                None,
            )
            .await
            .map(|s| s.trim() != "0")
            .unwrap_or(false);
        Ok(status)
    }

    /// Fetch the primary mirror (refresh `origin/*` refs seen by worktrees).
    pub async fn fetch(&self, spec: &WorkspaceSpec) -> Result<(), WebPlatformError> {
        let mirror = self.mirror_dir(spec.project_id, spec.primary.alias.as_deref());
        self.git(
            &mirror,
            &["fetch", "--prune", "origin"],
            spec.token.as_deref(),
        )
        .await
        .map(|_| ())
    }

    /// Merge the latest default branch into the work branch (no rebase).
    pub async fn merge_main(&self, spec: &WorkspaceSpec) -> Result<(), WebPlatformError> {
        self.fetch(spec).await?;
        let dir = self.primary_dir(spec);
        let base = format!("origin/{}", spec.primary.default_branch);
        if let Err(e) = self.git(&dir, &["merge", "--no-edit", &base], None).await {
            let _ = self.git(&dir, &["merge", "--abort"], None).await;
            return Err(WebPlatformError::Conflict(format!(
                "合并主干时发生冲突，需要人工处理：{e}"
            )));
        }
        Ok(())
    }

    // ----------------------------------------------------------- path safety

    /// Resolve a workspace-relative path; rejects absolute paths, `..`,
    /// `.git` components and symlink escapes.
    pub fn resolve(&self, spec: &WorkspaceSpec, rel: &str) -> Result<PathBuf, WebPlatformError> {
        let ws = self.workspace_dir(spec.project_id, &spec.key);
        let rel = rel.trim_start_matches("./");
        let p = Path::new(rel);
        if rel.is_empty() {
            return Ok(ws);
        }
        for c in p.components() {
            match c {
                Component::Normal(s) if s == ".git" => {
                    return Err(WebPlatformError::BadRequest("不能访问 .git 目录".into()))
                }
                Component::Normal(_) => {}
                _ => return Err(WebPlatformError::BadRequest(format!("非法路径：{rel}"))),
            }
        }
        let full = ws.join(p);
        // Symlink escape check on the deepest existing ancestor.
        let mut probe = full.clone();
        while !probe.exists() {
            if !probe.pop() {
                break;
            }
        }
        if let (Ok(real), Ok(real_ws)) = (probe.canonicalize(), ws.canonicalize()) {
            if !real.starts_with(&real_ws) {
                return Err(WebPlatformError::BadRequest(format!("路径越界：{rel}")));
            }
        }
        Ok(full)
    }

    /// Glob-free white-list check: `writable` entries are directory prefixes.
    pub fn is_writable(writable: &[String], rel: &str) -> bool {
        let rel = rel.trim_start_matches("./");
        writable.iter().any(|w| {
            let w = w.trim_end_matches("/**").trim_end_matches('/');
            rel == w || rel.starts_with(&format!("{w}/"))
        })
    }

    // ------------------------------------------------------------ operations

    pub async fn list_dir(
        &self,
        spec: &WorkspaceSpec,
        rel: &str,
        writable: &[String],
    ) -> Result<Vec<TreeEntry>, WebPlatformError> {
        let dir = self.resolve(spec, rel)?;
        let mut rd = tokio::fs::read_dir(&dir)
            .await
            .map_err(|_| WebPlatformError::NotFound(format!("目录不存在：{rel}")))?;
        let mut names = Vec::new();
        while let Ok(Some(entry)) = rd.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".git" {
                continue;
            }
            names.push((name, entry));
        }
        // Respect .gitignore (per containing repository).
        let rels: Vec<String> = names.iter().map(|(n, _)| join_rel(rel, n)).collect();
        let ignored = self.ignored(spec, &rels).await;
        let mut out = Vec::new();
        for ((name, entry), path) in names.into_iter().zip(rels) {
            if ignored.contains(&path) {
                continue;
            }
            let meta = match entry.metadata().await {
                Ok(m) => m,
                Err(_) => continue,
            };
            let kind = if meta.is_dir() { "dir" } else { "file" };
            out.push(TreeEntry {
                writable: Self::is_writable(writable, &path),
                size: meta.is_file().then_some(meta.len()),
                name,
                path,
                kind: kind.to_string(),
            });
        }
        out.sort_by_key(|e| (e.kind != "dir", e.name.to_lowercase()));
        Ok(out)
    }

    /// Which of `paths` are git-ignored (checked inside their own repository).
    async fn ignored(&self, spec: &WorkspaceSpec, paths: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for p in paths {
            let (repo_dir, inner) = self.split_repo(spec, p);
            if inner.is_empty() {
                continue;
            }
            if self
                .git(&repo_dir, &["check-ignore", "-q", &inner], None)
                .await
                .is_ok()
            {
                out.push(p.clone());
            }
        }
        out
    }

    /// Split a workspace-relative path into (repository dir, path inside it).
    pub fn split_repo(&self, spec: &WorkspaceSpec, rel: &str) -> (PathBuf, String) {
        let ws = self.workspace_dir(spec.project_id, &spec.key);
        if spec.is_multi_repo() {
            let mut parts = rel.splitn(2, '/');
            let alias = parts.next().unwrap_or_default();
            let rest = parts.next().unwrap_or_default();
            (ws.join(alias), rest.to_string())
        } else {
            (ws, rel.to_string())
        }
    }

    pub async fn read_file(
        &self,
        spec: &WorkspaceSpec,
        rel: &str,
        writable: &[String],
    ) -> Result<FileContent, WebPlatformError> {
        let path = self.resolve(spec, rel)?;
        let meta = tokio::fs::metadata(&path)
            .await
            .map_err(|_| WebPlatformError::NotFound(format!("文件不存在：{rel}")))?;
        if meta.is_dir() {
            return Err(WebPlatformError::BadRequest(format!("{rel} 是目录")));
        }
        let size = meta.len();
        let writable = Self::is_writable(writable, rel);
        if size > MAX_TEXT_FILE {
            return Ok(FileContent {
                path: rel.to_string(),
                content: None,
                version: String::new(),
                size,
                binary: false,
                too_large: true,
                writable: false,
            });
        }
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|e| WebPlatformError::Internal(e.to_string()))?;
        let version = content_version(&bytes);
        let binary = bytes.iter().take(8000).any(|b| *b == 0);
        let content = if binary {
            None
        } else {
            Some(String::from_utf8_lossy(&bytes).to_string())
        };
        Ok(FileContent {
            path: rel.to_string(),
            content,
            version,
            size,
            binary,
            too_large: false,
            writable: writable && !binary,
        })
    }

    /// Content of a file at a git revision (`None` when absent at that rev).
    pub async fn read_at(
        &self,
        spec: &WorkspaceSpec,
        rel: &str,
        rev: &str,
    ) -> Result<Option<String>, WebPlatformError> {
        self.resolve(spec, rel)?;
        if !rev.chars().all(|c| c.is_ascii_hexdigit()) || rev.len() < 7 || rev.len() > 64 {
            return Err(WebPlatformError::BadRequest("rev 必须是 commit sha".into()));
        }
        let (dir, inner) = self.split_repo(spec, rel);
        match self
            .git(&dir, &["show", &format!("{rev}:{inner}")], None)
            .await
        {
            Ok(s) => Ok(Some(s)),
            Err(_) => Ok(None),
        }
    }

    /// Version of a file, or `None` when it does not exist.
    pub async fn file_version(
        &self,
        spec: &WorkspaceSpec,
        rel: &str,
    ) -> Result<Option<String>, WebPlatformError> {
        let path = self.resolve(spec, rel)?;
        match tokio::fs::read(&path).await {
            Ok(b) => Ok(Some(content_version(&b))),
            Err(_) => Ok(None),
        }
    }

    /// Write with optimistic locking. `base_version` `None` means "create";
    /// fails with 409 if the file exists.
    pub async fn write_file(
        &self,
        spec: &WorkspaceSpec,
        rel: &str,
        content: &str,
        base_version: Option<&str>,
        writable: &[String],
    ) -> Result<String, WebPlatformError> {
        if !Self::is_writable(writable, rel) {
            return Err(WebPlatformError::ForbiddenWithReason(format!(
                "{rel} 不在可写范围内"
            )));
        }
        let path = self.resolve(spec, rel)?;
        let current = self.file_version(spec, rel).await?;
        match (current.as_deref(), base_version) {
            (Some(cur), Some(base)) if cur != base => {
                return Err(WebPlatformError::Conflict(
                    "文件已被修改，请查看差异后再保存".to_string(),
                ))
            }
            (Some(_), None) => return Err(WebPlatformError::Conflict("文件已存在".to_string())),
            (None, Some(_)) => return Err(WebPlatformError::Conflict("文件已被删除".to_string())),
            _ => {}
        }
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| WebPlatformError::Internal(e.to_string()))?;
        }
        // Write through a temp file so readers never see half a file.
        let tmp = path.with_extension("symphony-tmp");
        tokio::fs::write(&tmp, content.as_bytes())
            .await
            .map_err(|e| WebPlatformError::Internal(e.to_string()))?;
        tokio::fs::rename(&tmp, &path)
            .await
            .map_err(|e| WebPlatformError::Internal(e.to_string()))?;
        Ok(content_version(content.as_bytes()))
    }

    pub async fn search(
        &self,
        spec: &WorkspaceSpec,
        query: &str,
        mode: &str,
    ) -> Result<Vec<SearchHit>, WebPlatformError> {
        let q = query.trim();
        if q.is_empty() || q.len() > 200 {
            return Err(WebPlatformError::BadRequest("搜索词长度需为 1-200".into()));
        }
        let ws = self.workspace_dir(spec.project_id, &spec.key);
        let mut repos: Vec<(String, PathBuf)> = vec![(spec.primary_rel(), self.primary_dir(spec))];
        for r in &spec.secondaries {
            if let Some(a) = &r.alias {
                repos.push((a.clone(), ws.join(a)));
            }
        }
        let mut hits = Vec::new();
        for (prefix, dir) in repos {
            if !dir.join(".git").exists() {
                continue;
            }
            let prefixed = |p: &str| {
                if prefix.is_empty() {
                    p.to_string()
                } else {
                    format!("{prefix}/{p}")
                }
            };
            if mode == "file" {
                let out = self
                    .git(&dir, &["ls-files"], None)
                    .await
                    .unwrap_or_default();
                let ql = q.to_lowercase();
                for line in out.lines() {
                    if fuzzy_match(&line.to_lowercase(), &ql) {
                        hits.push(SearchHit {
                            path: prefixed(line),
                            line: None,
                            text: None,
                        });
                        if hits.len() >= MAX_SEARCH_RESULTS {
                            return Ok(hits);
                        }
                    }
                }
            } else {
                // `git grep` respects .gitignore and needs no extra binary.
                let out = self
                    .git(
                        &dir,
                        &["grep", "-n", "-I", "-F", "--untracked", "-e", q],
                        None,
                    )
                    .await
                    .unwrap_or_default();
                for line in out.lines() {
                    let mut parts = line.splitn(3, ':');
                    let (Some(p), Some(n), Some(t)) = (parts.next(), parts.next(), parts.next())
                    else {
                        continue;
                    };
                    hits.push(SearchHit {
                        path: prefixed(p),
                        line: n.parse().ok(),
                        text: Some(t.chars().take(300).collect()),
                    });
                    if hits.len() >= MAX_SEARCH_RESULTS {
                        return Ok(hits);
                    }
                }
            }
        }
        Ok(hits)
    }

    /// Paths (relative to the primary repo) changed in the working tree.
    pub async fn changed_paths(
        &self,
        spec: &WorkspaceSpec,
    ) -> Result<Vec<String>, WebPlatformError> {
        let dir = self.primary_dir(spec);
        let out = self
            .git(
                &dir,
                &["status", "--porcelain", "--untracked-files=all"],
                None,
            )
            .await?;
        Ok(out
            .lines()
            .filter_map(|l| l.get(3..))
            .map(|p| {
                p.rsplit(" -> ")
                    .next()
                    .unwrap_or(p)
                    .trim_matches('"')
                    .to_string()
            })
            .collect())
    }

    /// Restore paths outside the white-list (AI out-of-bounds residue).
    /// `writable_in_repo` is relative to the primary repository.
    pub async fn discard_outside(
        &self,
        spec: &WorkspaceSpec,
        writable_in_repo: &str,
    ) -> Result<Vec<String>, WebPlatformError> {
        let dir = self.primary_dir(spec);
        let mut discarded = Vec::new();
        for p in self.changed_paths(spec).await? {
            if p == writable_in_repo || p.starts_with(&format!("{writable_in_repo}/")) {
                continue;
            }
            let tracked = self
                .git(&dir, &["ls-files", "--error-unmatch", &p], None)
                .await
                .is_ok();
            if tracked {
                self.git(&dir, &["checkout", "--", &p], None).await?;
            } else {
                let full = dir.join(&p);
                let _ = tokio::fs::remove_file(&full).await;
            }
            discarded.push(p);
        }
        // Secondary repositories must stay pristine.
        let ws = self.workspace_dir(spec.project_id, &spec.key);
        for r in &spec.secondaries {
            if let Some(a) = &r.alias {
                let d = ws.join(a);
                if d.join(".git").exists() {
                    let out = self
                        .git(&d, &["status", "--porcelain"], None)
                        .await
                        .unwrap_or_default();
                    if !out.trim().is_empty() {
                        let _ = self.git(&d, &["checkout", "--", "."], None).await;
                        let _ = self.git(&d, &["clean", "-fd"], None).await;
                        discarded.push(format!("{a}/*"));
                    }
                }
            }
        }
        Ok(discarded)
    }

    /// Commit only `paths_in_repo` (relative to primary) and push. Returns the
    /// new HEAD sha, or `None` when there was nothing to commit.
    pub async fn commit_and_push(
        &self,
        spec: &WorkspaceSpec,
        dir_in_repo: &str,
        message: &str,
        actor_trailer: &str,
    ) -> Result<Option<String>, WebPlatformError> {
        let dir = self.primary_dir(spec);
        self.git(&dir, &["add", "--all", "--", dir_in_repo], None)
            .await?;
        let staged = self
            .git(&dir, &["diff", "--cached", "--name-only"], None)
            .await?;
        if staged.trim().is_empty() {
            return Ok(None);
        }
        let msg = format!("{message}\n\nSymphony-Actor: {actor_trailer}\n");
        self.git(&dir, &["commit", "-q", "-m", &msg], None).await?;
        self.push(spec).await?;
        let sha = self.git(&dir, &["rev-parse", "HEAD"], None).await?;
        Ok(Some(sha.trim().to_string()))
    }

    pub async fn push(&self, spec: &WorkspaceSpec) -> Result<(), WebPlatformError> {
        let dir = self.primary_dir(spec);
        let refspec = format!("HEAD:refs/heads/{}", spec.branch);
        self.git(&dir, &["push", "origin", &refspec], spec.token.as_deref())
            .await
            .map(|_| ())
    }

    pub async fn head_sha(&self, spec: &WorkspaceSpec) -> Result<String, WebPlatformError> {
        let dir = self.primary_dir(spec);
        Ok(self
            .git(&dir, &["rev-parse", "HEAD"], None)
            .await?
            .trim()
            .to_string())
    }

    /// Unified diff of the primary repo work tree against HEAD for a directory.
    pub async fn diff(
        &self,
        spec: &WorkspaceSpec,
        dir_in_repo: &str,
    ) -> Result<String, WebPlatformError> {
        let dir = self.primary_dir(spec);
        let _ = self
            .git(&dir, &["add", "-N", "--", dir_in_repo], None)
            .await;
        self.git(&dir, &["diff", "HEAD", "--", dir_in_repo], None)
            .await
    }

    pub async fn remove(&self, spec: &WorkspaceSpec) -> Result<(), WebPlatformError> {
        let ws = self.workspace_dir(spec.project_id, &spec.key);
        let _ = tokio::fs::remove_dir_all(&ws).await;
        let mirror = self.mirror_dir(spec.project_id, spec.primary.alias.as_deref());
        let _ = self.git(&mirror, &["worktree", "prune"], None).await;
        Ok(())
    }
}

fn join_rel(base: &str, name: &str) -> String {
    let base = base.trim_matches('/');
    if base.is_empty() {
        name.to_string()
    } else {
        format!("{base}/{name}")
    }
}

/// Subsequence match ("srcmain" matches "src/main.rs").
fn fuzzy_match(hay: &str, needle: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|c| it.any(|h| h == c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writable_prefix_rules() {
        let w = vec!["docs/changes/12/**".to_string()];
        assert!(WorkspaceService::is_writable(&w, "docs/changes/12/spec.md"));
        assert!(WorkspaceService::is_writable(&w, "docs/changes/12"));
        assert!(!WorkspaceService::is_writable(
            &w,
            "docs/changes/123/spec.md"
        ));
        assert!(!WorkspaceService::is_writable(&w, "src/main.rs"));
        assert!(!WorkspaceService::is_writable(&[], "a"));
    }

    #[test]
    fn fuzzy() {
        assert!(fuzzy_match("src/main.rs", "srcmain"));
        assert!(!fuzzy_match("src/main.rs", "zz"));
    }

    fn spec(dir: &Path) -> (WorkspaceService, WorkspaceSpec) {
        let svc = WorkspaceService::new(dir);
        let s = WorkspaceSpec {
            project_id: 1,
            key: "7".into(),
            primary: RepoCheckout {
                alias: None,
                clone_url: String::new(),
                default_branch: "main".into(),
                writable: true,
            },
            secondaries: vec![],
            branch: "b".into(),
            token: None,
        };
        (svc, s)
    }

    #[test]
    fn resolve_rejects_escape() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, s) = spec(dir.path());
        assert!(svc.resolve(&s, "../x").is_err());
        assert!(svc.resolve(&s, "/etc/passwd").is_err());
        assert!(svc.resolve(&s, "a/.git/config").is_err());
        assert!(svc.resolve(&s, "a/b.md").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let (svc, s) = spec(dir.path());
        let ws = svc.workspace_dir(1, "7");
        std::fs::create_dir_all(&ws).unwrap();
        std::os::unix::fs::symlink(outside.path(), ws.join("link")).unwrap();
        assert!(svc.resolve(&s, "link/secret").is_err());
    }

    #[test]
    fn split_repo_multi() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, mut s) = spec(dir.path());
        assert_eq!(svc.split_repo(&s, "a/b").1, "a/b");
        s.primary.alias = Some("frontend".into());
        s.secondaries.push(RepoCheckout {
            alias: Some("backend".into()),
            clone_url: String::new(),
            default_branch: "main".into(),
            writable: false,
        });
        let (d, inner) = svc.split_repo(&s, "backend/src/x.rs");
        assert!(d.ends_with("backend"));
        assert_eq!(inner, "src/x.rs");
        assert_eq!(s.primary_rel(), "frontend");
    }
}

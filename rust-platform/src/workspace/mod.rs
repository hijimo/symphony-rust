//! Workspace Manager — per-issue workspace lifecycle management.
//!
//! Manages creation, reuse, hook execution, cleanup, and safety invariants
//! for per-issue workspace directories.
//!
//! SPEC reference: Section 9

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use thiserror::Error;
use tokio::process::Command;

use crate::config::service_config::HooksConfig;

/// Workspace information (SPEC Section 4.1.4).
#[derive(Debug, Clone)]
pub struct Workspace {
    pub path: PathBuf,
    pub workspace_key: String,
    pub created_now: bool,
}

/// Errors from workspace operations.
#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("unsafe workspace identifier: {identifier}")]
    UnsafeIdentifier { identifier: String },

    #[error("workspace path {path:?} is outside root {root:?}")]
    PathOutsideRoot { path: PathBuf, root: PathBuf },

    #[error("failed to create workspace directory: {source}")]
    CreateFailed {
        #[source]
        source: std::io::Error,
    },

    #[error("hook '{hook}' failed with exit code {exit_code:?}")]
    HookFailed {
        hook: String,
        exit_code: Option<i32>,
    },

    #[error("hook '{hook}' timed out")]
    HookTimeout { hook: String },

    #[error("hook I/O error: {source}")]
    HookIo {
        #[source]
        source: std::io::Error,
    },

    #[error("failed to remove workspace: {source}")]
    RemoveFailed {
        #[source]
        source: std::io::Error,
    },

    #[error("I/O error: {source}")]
    Io {
        #[source]
        source: std::io::Error,
    },

    #[error("invalid workspace root: {0}")]
    InvalidRoot(String),
}

/// Sanitize an issue identifier into a safe workspace directory name (SPEC Section 4.2).
///
/// Replaces any character not in `[A-Za-z0-9._-]` with `_`.
/// No merging of consecutive underscores, no trimming.
/// Rejects empty strings, `.`, `..`, and strings consisting entirely of dots.
pub fn sanitize_workspace_key(identifier: &str) -> Result<String, WorkspaceError> {
    let sanitized: String = identifier
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();

    // Safety: reject empty, ".", "..", and all-dots strings (path traversal prevention)
    if sanitized.is_empty()
        || sanitized == "."
        || sanitized == ".."
        || sanitized.chars().all(|c| c == '.')
    {
        return Err(WorkspaceError::UnsafeIdentifier {
            identifier: identifier.to_string(),
        });
    }

    Ok(sanitized)
}

/// Workspace Manager — owns the workspace root and hooks config.
///
/// Responsible for creating/reusing per-issue workspaces, running lifecycle hooks,
/// validating path containment, and cleaning up terminal workspaces.
pub struct WorkspaceManager {
    root: PathBuf,
    hooks: HooksConfig,
}

impl WorkspaceManager {
    /// Create a new WorkspaceManager with the given root path and hooks config.
    pub fn new(root: PathBuf, hooks: HooksConfig) -> Self {
        Self { root, hooks }
    }

    /// Get the workspace root path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Update hooks configuration (for hot-reload support).
    pub fn update_hooks(&mut self, hooks: HooksConfig) {
        self.hooks = hooks;
    }

    /// Create or reuse a workspace for the given issue identifier (SPEC Section 9.2).
    ///
    /// Algorithm:
    /// 1. Sanitize identifier to workspace_key
    /// 2. Compute workspace path under root
    /// 3. Create directory if it doesn't exist
    /// 4. Validate path containment (SPEC Section 9.5 Invariant 2)
    /// 5. Run after_create hook if directory was newly created
    pub async fn ensure_workspace(&self, identifier: &str) -> Result<Workspace, WorkspaceError> {
        let key = sanitize_workspace_key(identifier)?;
        let path = self.root.join(&key);

        let created_now = if path.is_dir() {
            false
        } else {
            tokio::fs::create_dir_all(&path)
                .await
                .map_err(|e| WorkspaceError::CreateFailed { source: e })?;
            true
        };

        // Safety: validate path containment after directory creation so canonicalize works
        self.validate_path_containment(&path)?;

        // Run after_create hook only for newly created workspaces
        if created_now {
            if let Some(ref script) = self.hooks.after_create {
                if let Err(e) = self.run_hook("after_create", script, &path).await {
                    // after_create failure is fatal — remove the partially created workspace
                    let _ = tokio::fs::remove_dir_all(&path).await;
                    return Err(e);
                }
            }
        }

        Ok(Workspace {
            path,
            workspace_key: key,
            created_now,
        })
    }

    /// Validate that a workspace path is contained within the workspace root (SPEC Section 9.5).
    ///
    /// Uses canonicalize to resolve symlinks and ensure the real path is under root.
    /// Must be called after directory creation so canonicalize can resolve the path.
    pub fn validate_path_containment(&self, path: &Path) -> Result<(), WorkspaceError> {
        let canonical_root = self
            .root
            .canonicalize()
            .map_err(|e| WorkspaceError::Io { source: e })?;
        let canonical_path = path
            .canonicalize()
            .map_err(|_| WorkspaceError::PathOutsideRoot {
                path: path.to_path_buf(),
                root: canonical_root.clone(),
            })?;

        if !canonical_path.starts_with(&canonical_root) {
            return Err(WorkspaceError::PathOutsideRoot {
                path: canonical_path,
                root: canonical_root,
            });
        }

        Ok(())
    }

    /// Execute a shell hook script with timeout (SPEC Section 9.4).
    ///
    /// Runs `bash -lc <script>` with the workspace directory as cwd.
    /// Applies `hooks.timeout_ms` as the execution deadline.
    pub async fn run_hook(
        &self,
        name: &str,
        script: &str,
        cwd: &Path,
    ) -> Result<(), WorkspaceError> {
        let timeout = Duration::from_millis(self.hooks.timeout_ms);

        tracing::info!(hook = name, cwd = %cwd.display(), "running workspace hook");

        let mut child = Command::new("bash")
            .args(["-lc", script])
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| WorkspaceError::HookIo { source: e })?;

        // Take stdout/stderr handles before the select! to avoid ownership issues
        let _stdout = child.stdout.take();
        let _stderr = child.stderr.take();

        tokio::select! {
            result = child.wait() => {
                match result {
                    Ok(status) if status.success() => {
                        tracing::info!(hook = name, "hook completed successfully");
                        Ok(())
                    }
                    Ok(status) => {
                        tracing::error!(hook = name, exit_code = ?status.code(), "hook failed");
                        Err(WorkspaceError::HookFailed {
                            hook: name.to_string(),
                            exit_code: status.code(),
                        })
                    }
                    Err(e) => Err(WorkspaceError::HookIo { source: e }),
                }
            }
            _ = tokio::time::sleep(timeout) => {
                tracing::error!(hook = name, timeout_ms = self.hooks.timeout_ms, "hook timed out");
                // Timeout: kill the child process
                let _ = child.kill().await;
                // Wait to reap the zombie process
                let _ = child.wait().await;
                Err(WorkspaceError::HookTimeout { hook: name.to_string() })
            }
        }
    }

    /// Run the before_run hook (SPEC Section 9.4).
    ///
    /// Failure is fatal to the current run attempt.
    pub async fn run_before_run(&self, workspace_path: &Path) -> Result<(), WorkspaceError> {
        if let Some(ref script) = self.hooks.before_run {
            self.run_hook("before_run", script, workspace_path).await?;
        }
        Ok(())
    }

    /// Run the after_run hook (SPEC Section 9.4).
    ///
    /// Failure is logged and ignored — does not affect the run outcome.
    pub async fn run_after_run(&self, workspace_path: &Path) {
        if let Some(ref script) = self.hooks.after_run {
            if let Err(e) = self.run_hook("after_run", script, workspace_path).await {
                tracing::warn!(error = %e, "after_run hook failed (ignored)");
            }
        }
    }

    /// Remove a workspace directory for the given identifier (SPEC Section 8.6).
    ///
    /// Runs before_remove hook first (failure is logged and ignored).
    /// Then removes the directory.
    pub async fn remove_workspace(&self, identifier: &str) {
        let key = match sanitize_workspace_key(identifier) {
            Ok(k) => k,
            Err(e) => {
                tracing::warn!(identifier, error = %e, "cannot sanitize identifier for removal");
                return;
            }
        };

        let path = self.root.join(&key);
        if !path.is_dir() {
            return;
        }

        // Run before_remove hook (failure is logged and ignored)
        if let Some(ref script) = self.hooks.before_remove {
            if let Err(e) = self.run_hook("before_remove", script, &path).await {
                tracing::warn!(error = %e, "before_remove hook failed (ignored)");
            }
        }

        // Remove the workspace directory
        if let Err(e) = tokio::fs::remove_dir_all(&path).await {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "failed to remove workspace directory"
            );
        } else {
            tracing::info!(identifier, path = %path.display(), "workspace removed");
        }
    }

    /// Clean up workspaces for issues in terminal states (SPEC Section 8.6).
    ///
    /// Called at startup to prevent stale terminal workspaces from accumulating.
    pub async fn cleanup_terminal_workspaces(&self, identifiers: &[String]) {
        for identifier in identifiers {
            self.remove_workspace(identifier).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_workspace_key_basic() {
        assert_eq!(sanitize_workspace_key("ABC-123").unwrap(), "ABC-123");
        assert_eq!(sanitize_workspace_key("my.issue").unwrap(), "my.issue");
        assert_eq!(sanitize_workspace_key("test_key").unwrap(), "test_key");
    }

    #[test]
    fn test_sanitize_workspace_key_replaces_special_chars() {
        assert_eq!(sanitize_workspace_key("ABC/123").unwrap(), "ABC_123");
        assert_eq!(sanitize_workspace_key("a b c").unwrap(), "a_b_c");
        assert_eq!(
            sanitize_workspace_key("foo@bar#baz").unwrap(),
            "foo_bar_baz"
        );
    }

    #[test]
    fn test_sanitize_workspace_key_rejects_unsafe() {
        assert!(sanitize_workspace_key("").is_err());
        assert!(sanitize_workspace_key(".").is_err());
        assert!(sanitize_workspace_key("..").is_err());
        assert!(sanitize_workspace_key("...").is_err());
        // A slash-only string becomes all underscores, which is valid
        assert!(sanitize_workspace_key("/").is_ok());
    }

    #[test]
    fn test_sanitize_workspace_key_unicode() {
        // Unicode chars get replaced with underscore
        assert_eq!(sanitize_workspace_key("日本語").unwrap(), "___");
    }

    #[tokio::test]
    async fn test_ensure_workspace_creates_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());

        let ws = mgr.ensure_workspace("TEST-1").await.unwrap();
        assert!(ws.path.is_dir());
        assert!(ws.created_now);
        assert_eq!(ws.workspace_key, "TEST-1");
    }

    #[tokio::test]
    async fn test_ensure_workspace_reuses_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());

        let ws1 = mgr.ensure_workspace("TEST-2").await.unwrap();
        assert!(ws1.created_now);

        let ws2 = mgr.ensure_workspace("TEST-2").await.unwrap();
        assert!(!ws2.created_now);
        assert_eq!(ws1.path, ws2.path);
    }

    #[tokio::test]
    async fn test_remove_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());

        let ws = mgr.ensure_workspace("TEST-3").await.unwrap();
        assert!(ws.path.is_dir());

        mgr.remove_workspace("TEST-3").await;
        assert!(!ws.path.is_dir());
    }

    #[tokio::test]
    async fn test_path_containment_rejects_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());

        // A path outside the root should be rejected
        let outside_path = PathBuf::from("/tmp/outside_workspace");
        // This will fail because the path doesn't exist (can't canonicalize)
        assert!(mgr.validate_path_containment(&outside_path).is_err());
    }

    #[tokio::test]
    async fn test_hook_execution_success() {
        let tmp = tempfile::tempdir().unwrap();
        let hooks = HooksConfig {
            after_create: Some("echo 'created'".to_string()),
            before_run: Some("echo 'before'".to_string()),
            after_run: Some("echo 'after'".to_string()),
            before_remove: None,
            timeout_ms: 5_000,
        };
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), hooks);

        let ws = mgr.ensure_workspace("HOOK-1").await.unwrap();
        assert!(ws.created_now);

        // before_run should succeed
        mgr.run_before_run(&ws.path).await.unwrap();
    }

    #[tokio::test]
    async fn test_hook_failure_is_fatal_for_after_create() {
        let tmp = tempfile::tempdir().unwrap();
        let hooks = HooksConfig {
            after_create: Some("exit 1".to_string()),
            before_run: None,
            after_run: None,
            before_remove: None,
            timeout_ms: 5_000,
        };
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), hooks);

        let result = mgr.ensure_workspace("FAIL-1").await;
        assert!(result.is_err());
        // The workspace directory should have been cleaned up
        assert!(!tmp.path().join("FAIL-1").is_dir());
    }
}

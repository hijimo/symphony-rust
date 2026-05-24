//! Workspace Manager — per-issue workspace lifecycle management.
//!
//! Manages creation, reuse, hook execution, cleanup, and safety invariants
//! for per-issue workspace directories.
//!
//! SPEC reference: Section 9

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::service_config::HooksConfig;
use crate::proxy::proxy_command;

/// Workspace information (SPEC Section 4.1.4).
#[derive(Debug, Clone)]
pub struct Workspace {
    pub path: PathBuf,
    pub workspace_key: String,
    pub created_now: bool,
}

/// Lease returned by the issue-id keyed workspace preparation path.
#[derive(Debug, Clone)]
pub struct WorkspaceRunLease {
    pub workspace: Workspace,
    pub issue_id_path_key: String,
    pub metadata_path: PathBuf,
    pub lock_path: PathBuf,
    pub lock_sidecar_path: PathBuf,
    _lock_guard: Arc<std::fs::File>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspaceMetadata {
    schema_version: u32,
    issue_id: String,
    issue_id_path_key: String,
    workspace_key: String,
    issue_identifier: String,
    init_started_at: DateTime<Utc>,
    initialized_at: Option<DateTime<Utc>>,
    init_status: String,
    after_create_fingerprint: Option<String>,
    initialized_by_run_id: String,
    expected_git_root: String,
    last_error: Option<WorkspaceMetadataError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspaceMetadataError {
    code: String,
    message: String,
    at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LockSidecar {
    schema_version: u32,
    issue_id: String,
    issue_id_path_key: String,
    workspace_key: String,
    run_id: String,
    service_instance_id: String,
    workspace_path: String,
    created_at: DateTime<Utc>,
    updated_seq: u64,
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

    #[error("workspace requires manual recovery: {reason}")]
    ManualRecoveryRequired { reason: String },

    #[error("workspace metadata error: {reason}")]
    MetadataInvalid { reason: String },
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

    /// Encode a raw tracker issue id into a path-safe, case-folding-safe key.
    pub fn issue_id_path_key(issue_id: &str) -> String {
        let mut encoded = String::with_capacity(2 + issue_id.len() * 2);
        encoded.push_str("i-");
        for byte in issue_id.as_bytes() {
            encoded.push(hex_digit(byte >> 4));
            encoded.push(hex_digit(byte & 0x0f));
        }
        encoded
    }

    /// Prepare an issue workspace using stable issue identity rather than
    /// identifier-only paths.
    pub async fn prepare_issue_workspace(
        &self,
        issue_id: &str,
        issue_identifier: &str,
        run_id: &str,
        service_instance_id: &str,
    ) -> Result<WorkspaceRunLease, WorkspaceError> {
        let issue_id_path_key = Self::issue_id_path_key(issue_id);
        let identifier_key = sanitize_workspace_key(issue_identifier)?;
        let workspace_key = format!("{issue_id_path_key}-{identifier_key}");
        let workspace_path = self.root.join(&workspace_key);
        let metadata_path = workspace_path.join(".symphony-workspace.json");
        let locks_dir = self.root.join(".symphony").join("locks").join("issues");
        tokio::fs::create_dir_all(&locks_dir)
            .await
            .map_err(|e| WorkspaceError::Io { source: e })?;
        let lock_path = locks_dir.join(format!("{issue_id_path_key}.lock"));
        let lock_sidecar_path = locks_dir.join(format!("{issue_id_path_key}.json"));

        let lock_guard = acquire_stable_lock_file(&lock_path).await?;

        let legacy_path = self.root.join(&identifier_key);
        if legacy_path.exists()
            && legacy_path != workspace_path
            && !is_dir_empty(&legacy_path).await?
        {
            return Err(WorkspaceError::ManualRecoveryRequired {
                reason: "workspace_legacy_requires_manual".to_string(),
            });
        }

        let created_now = if workspace_path.is_dir() {
            false
        } else {
            tokio::fs::create_dir_all(&workspace_path)
                .await
                .map_err(|e| WorkspaceError::CreateFailed { source: e })?;
            true
        };

        self.validate_path_containment(&workspace_path)?;

        if metadata_path.exists() {
            let metadata = read_workspace_metadata(&metadata_path).await?;
            validate_workspace_metadata(&metadata, issue_id, &issue_id_path_key, &workspace_key)?;
            if metadata.init_status == "ready" {
                write_lock_sidecar(
                    &lock_sidecar_path,
                    LockSidecar {
                        schema_version: 1,
                        issue_id: issue_id.to_string(),
                        issue_id_path_key: issue_id_path_key.clone(),
                        workspace_key: workspace_key.clone(),
                        run_id: run_id.to_string(),
                        service_instance_id: service_instance_id.to_string(),
                        workspace_path: workspace_path.display().to_string(),
                        created_at: Utc::now(),
                        updated_seq: 1,
                    },
                )
                .await?;
                return Ok(WorkspaceRunLease {
                    workspace: Workspace {
                        path: workspace_path,
                        workspace_key,
                        created_now,
                    },
                    issue_id_path_key,
                    metadata_path,
                    lock_path,
                    lock_sidecar_path,
                    _lock_guard: lock_guard,
                });
            }
            return Err(WorkspaceError::ManualRecoveryRequired {
                reason: format!("workspace_not_ready:{}", metadata.init_status),
            });
        }

        if !created_now && !is_dir_empty(&workspace_path).await? {
            return Err(WorkspaceError::ManualRecoveryRequired {
                reason: "workspace_unknown_requires_manual".to_string(),
            });
        }

        let started_at = Utc::now();
        write_workspace_metadata(
            &metadata_path,
            WorkspaceMetadata {
                schema_version: 1,
                issue_id: issue_id.to_string(),
                issue_id_path_key: issue_id_path_key.clone(),
                workspace_key: workspace_key.clone(),
                issue_identifier: issue_identifier.to_string(),
                init_started_at: started_at,
                initialized_at: None,
                init_status: "initializing".to_string(),
                after_create_fingerprint: None,
                initialized_by_run_id: run_id.to_string(),
                expected_git_root: ".".to_string(),
                last_error: None,
            },
        )
        .await?;

        if let Some(ref script) = self.hooks.after_create {
            if let Err(e) = self.run_hook("after_create", script, &workspace_path).await {
                let failed_metadata = WorkspaceMetadata {
                    schema_version: 1,
                    issue_id: issue_id.to_string(),
                    issue_id_path_key: issue_id_path_key.clone(),
                    workspace_key: workspace_key.clone(),
                    issue_identifier: issue_identifier.to_string(),
                    init_started_at: started_at,
                    initialized_at: None,
                    init_status: "failed".to_string(),
                    after_create_fingerprint: None,
                    initialized_by_run_id: run_id.to_string(),
                    expected_git_root: ".".to_string(),
                    last_error: Some(WorkspaceMetadataError {
                        code: "after_create_failed".to_string(),
                        message: e.to_string(),
                        at: Utc::now(),
                    }),
                };
                let _ = write_workspace_metadata(&metadata_path, failed_metadata).await;
                quarantine_workspace(&self.root, &workspace_path, &issue_id_path_key, run_id)
                    .await?;
                return Err(e);
            }
        }

        write_workspace_metadata(
            &metadata_path,
            WorkspaceMetadata {
                schema_version: 1,
                issue_id: issue_id.to_string(),
                issue_id_path_key: issue_id_path_key.clone(),
                workspace_key: workspace_key.clone(),
                issue_identifier: issue_identifier.to_string(),
                init_started_at: started_at,
                initialized_at: Some(Utc::now()),
                init_status: "ready".to_string(),
                after_create_fingerprint: None,
                initialized_by_run_id: run_id.to_string(),
                expected_git_root: ".".to_string(),
                last_error: None,
            },
        )
        .await?;

        write_lock_sidecar(
            &lock_sidecar_path,
            LockSidecar {
                schema_version: 1,
                issue_id: issue_id.to_string(),
                issue_id_path_key: issue_id_path_key.clone(),
                workspace_key: workspace_key.clone(),
                run_id: run_id.to_string(),
                service_instance_id: service_instance_id.to_string(),
                workspace_path: workspace_path.display().to_string(),
                created_at: Utc::now(),
                updated_seq: 1,
            },
        )
        .await?;

        Ok(WorkspaceRunLease {
            workspace: Workspace {
                path: workspace_path,
                workspace_key,
                created_now: true,
            },
            issue_id_path_key,
            metadata_path,
            lock_path,
            lock_sidecar_path,
            _lock_guard: lock_guard,
        })
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

        let mut command = proxy_command("bash");
        command
            .args(["-lc", script])
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command
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

    /// Remove the canonical issue-id keyed workspace for an issue.
    pub async fn remove_issue_workspace(&self, issue_id: &str, identifier: &str) {
        let identifier_key = match sanitize_workspace_key(identifier) {
            Ok(k) => k,
            Err(e) => {
                tracing::warn!(issue_id, identifier, error = %e, "cannot sanitize identifier for removal");
                return;
            }
        };
        let workspace_key = format!("{}-{identifier_key}", Self::issue_id_path_key(issue_id));
        let path = self.root.join(&workspace_key);
        if !path.is_dir() {
            return;
        }

        if let Some(ref script) = self.hooks.before_remove {
            if let Err(e) = self.run_hook("before_remove", script, &path).await {
                tracing::warn!(error = %e, "before_remove hook failed (ignored)");
            }
        }

        if let Err(e) = tokio::fs::remove_dir_all(&path).await {
            tracing::warn!(
                issue_id,
                identifier,
                path = %path.display(),
                error = %e,
                "failed to remove issue workspace directory"
            );
        } else {
            tracing::info!(
                issue_id,
                identifier,
                path = %path.display(),
                "issue workspace removed"
            );
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

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + (nibble - 10)) as char,
        _ => unreachable!("nibble is masked to 4 bits"),
    }
}

async fn acquire_stable_lock_file(path: &Path) -> Result<Arc<std::fs::File>, WorkspaceError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| WorkspaceError::Io { source: e })?;
    }
    let path = path.to_path_buf();
    let file = tokio::task::spawn_blocking(move || {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        lock_file_exclusive(&file)?;
        Ok::<_, std::io::Error>(file)
    })
    .await
    .map_err(|e| WorkspaceError::Io {
        source: std::io::Error::other(e.to_string()),
    })?
    .map_err(|e| WorkspaceError::Io { source: e })?;
    Ok(Arc::new(file))
}

#[cfg(unix)]
fn lock_file_exclusive(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;

    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn lock_file_exclusive(_file: &std::fs::File) -> std::io::Result<()> {
    Ok(())
}

async fn is_dir_empty(path: &Path) -> Result<bool, WorkspaceError> {
    let mut entries = tokio::fs::read_dir(path)
        .await
        .map_err(|e| WorkspaceError::Io { source: e })?;
    Ok(entries
        .next_entry()
        .await
        .map_err(|e| WorkspaceError::Io { source: e })?
        .is_none())
}

async fn read_workspace_metadata(path: &Path) -> Result<WorkspaceMetadata, WorkspaceError> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| WorkspaceError::Io { source: e })?;
    serde_json::from_slice(&bytes).map_err(|e| WorkspaceError::MetadataInvalid {
        reason: e.to_string(),
    })
}

fn validate_workspace_metadata(
    metadata: &WorkspaceMetadata,
    issue_id: &str,
    issue_id_path_key: &str,
    workspace_key: &str,
) -> Result<(), WorkspaceError> {
    if metadata.schema_version != 1 {
        return Err(WorkspaceError::MetadataInvalid {
            reason: format!("unsupported schema {}", metadata.schema_version),
        });
    }
    if metadata.issue_id != issue_id
        || metadata.issue_id_path_key != issue_id_path_key
        || metadata.workspace_key != workspace_key
    {
        return Err(WorkspaceError::ManualRecoveryRequired {
            reason: "workspace_identity_mismatch".to_string(),
        });
    }
    Ok(())
}

async fn write_workspace_metadata(
    path: &Path,
    metadata: WorkspaceMetadata,
) -> Result<(), WorkspaceError> {
    let bytes =
        serde_json::to_vec_pretty(&metadata).map_err(|e| WorkspaceError::MetadataInvalid {
            reason: e.to_string(),
        })?;
    atomic_write(path, &bytes).await
}

async fn write_lock_sidecar(path: &Path, sidecar: LockSidecar) -> Result<(), WorkspaceError> {
    let bytes =
        serde_json::to_vec_pretty(&sidecar).map_err(|e| WorkspaceError::MetadataInvalid {
            reason: e.to_string(),
        })?;
    atomic_write(path, &bytes).await
}

async fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), WorkspaceError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| WorkspaceError::Io { source: e })?;
    }
    let unique = format!(
        "{}.{}.{}.tmp",
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("json"),
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    let tmp = path.with_extension(unique);
    tokio::fs::write(&tmp, bytes)
        .await
        .map_err(|e| WorkspaceError::Io { source: e })?;
    let file = tokio::fs::OpenOptions::new()
        .read(true)
        .open(&tmp)
        .await
        .map_err(|e| WorkspaceError::Io { source: e })?;
    file.sync_all()
        .await
        .map_err(|e| WorkspaceError::Io { source: e })?;
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|e| WorkspaceError::Io { source: e })?;
    Ok(())
}

async fn quarantine_workspace(
    root: &Path,
    workspace_path: &Path,
    issue_id_path_key: &str,
    run_id: &str,
) -> Result<(), WorkspaceError> {
    if !workspace_path.exists() {
        return Ok(());
    }
    let quarantine_dir = root
        .join(".symphony")
        .join("quarantine")
        .join(issue_id_path_key);
    tokio::fs::create_dir_all(&quarantine_dir)
        .await
        .map_err(|e| WorkspaceError::Io { source: e })?;
    let mut target = quarantine_dir.join(run_id);
    let mut suffix = 0usize;
    while target.exists() {
        suffix += 1;
        target = quarantine_dir.join(format!("{run_id}-{suffix}"));
    }
    tokio::fs::rename(workspace_path, target)
        .await
        .map_err(|e| WorkspaceError::Io { source: e })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::test_support::async_env_lock;

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
    async fn run_hook_uses_proxy_command_environment() {
        let mut env = async_env_lock().await;
        env.clear_proxy_env();
        let tmp = tempfile::tempdir().unwrap();
        let env_file = tmp.path().join("hook-env.txt");
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());
        env.set("SYMPHONY_PROXY_MODE", "inherit_env");
        env.set("SYMPHONY_PROXY_VERSION", "31");
        env.set("SYMPHONY_PROXY_SOURCE", "environment");
        env.set("ALL_PROXY", "http://proxy.example.com:8080");
        env.set("NO_PROXY", "localhost,127.0.0.1");
        env.set("HOOK_ENV_FILE", &env_file);

        mgr.run_hook("before_run", "env > \"$HOOK_ENV_FILE\"", tmp.path())
            .await
            .unwrap();
        let output = std::fs::read_to_string(&env_file).unwrap();

        assert!(output.contains("SYMPHONY_PROXY_MODE=inherit_env"));
        assert!(output.contains("ALL_PROXY=http://proxy.example.com:8080"));
        assert!(output.contains("all_proxy=http://proxy.example.com:8080"));
        assert!(output.contains("NO_PROXY=localhost,127.0.0.1"));
        assert!(output.contains("no_proxy=localhost,127.0.0.1"));
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

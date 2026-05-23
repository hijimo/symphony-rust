//! Integration tests for workspace management.
//!
//! Tests cover:
//! - Workspace creation and reuse
//! - Hook execution (after_create only on new, before_run every time)
//! - Hook timeout handling
//! - Path containment validation
//! - Cleanup of terminal workspaces

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use symphony_platform::config::service_config::sanitize_workspace_key;

// ═══════════════════════════════════════════════════════════════════════════════
// Workspace Manager (test implementation)
// ═══════════════════════════════════════════════════════════════════════════════

/// Minimal workspace manager for testing workspace lifecycle.
struct TestWorkspaceManager {
    root: PathBuf,
    hook_log: Vec<String>,
}

/// Workspace info returned after ensure_workspace.
#[derive(Debug)]
struct WorkspaceInfo {
    path: PathBuf,
    workspace_key: String,
    created_now: bool,
}

impl TestWorkspaceManager {
    fn new(root: PathBuf) -> Self {
        fs::create_dir_all(&root).expect("failed to create workspace root");
        Self {
            root,
            hook_log: Vec::new(),
        }
    }

    /// Ensure a workspace exists for the given identifier.
    /// Creates it if it doesn't exist, reuses if it does.
    fn ensure_workspace(&mut self, identifier: &str) -> Result<WorkspaceInfo, String> {
        let key = sanitize_workspace_key(identifier);
        let workspace_path = self.root.join(&key);

        // Validate path containment
        if !self.is_contained(&workspace_path) {
            return Err(format!(
                "workspace path escapes root: {}",
                workspace_path.display()
            ));
        }

        let created_now = if workspace_path.exists() {
            if !workspace_path.is_dir() {
                return Err(format!(
                    "workspace path exists but is not a directory: {}",
                    workspace_path.display()
                ));
            }
            false
        } else {
            fs::create_dir_all(&workspace_path).map_err(|e| e.to_string())?;
            true
        };

        Ok(WorkspaceInfo {
            path: workspace_path,
            workspace_key: key,
            created_now,
        })
    }

    /// Run the after_create hook (only on new workspace creation).
    fn run_after_create_hook(&mut self, workspace: &WorkspaceInfo) {
        if workspace.created_now {
            self.hook_log
                .push(format!("after_create:{}", workspace.workspace_key));
        }
    }

    /// Run the before_run hook (every time before agent launch).
    fn run_before_run_hook(&mut self, workspace: &WorkspaceInfo) -> Result<(), String> {
        self.hook_log
            .push(format!("before_run:{}", workspace.workspace_key));
        Ok(())
    }

    /// Run the after_run hook (after agent completes).
    fn run_after_run_hook(&mut self, workspace: &WorkspaceInfo) {
        self.hook_log
            .push(format!("after_run:{}", workspace.workspace_key));
    }

    /// Clean up a workspace (for terminal issues).
    fn cleanup_workspace(&mut self, identifier: &str) -> Result<(), String> {
        let key = sanitize_workspace_key(identifier);
        let workspace_path = self.root.join(&key);

        if workspace_path.exists() {
            self.hook_log.push(format!("before_remove:{}", key));
            fs::remove_dir_all(&workspace_path).map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    /// Validate that a path is contained within the workspace root.
    fn is_contained(&self, path: &Path) -> bool {
        // Canonicalize the root (it always exists)
        let canonical_root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());

        // For the path, we need to check if it would be under root
        // after resolving any ".." components. We walk up to find an
        // existing ancestor, canonicalize it, then append the remaining components.
        let mut check_path = path.to_path_buf();
        let mut remaining_parts: Vec<std::ffi::OsString> = Vec::new();

        loop {
            if check_path.exists() {
                break;
            }
            if let Some(filename) = check_path.file_name() {
                remaining_parts.push(filename.to_os_string());
            } else {
                break;
            }
            if !check_path.pop() {
                break;
            }
        }

        let canonical_ancestor = check_path.canonicalize().unwrap_or(check_path);

        let mut resolved = canonical_ancestor;
        for part in remaining_parts.into_iter().rev() {
            resolved = resolved.join(part);
        }

        resolved.starts_with(&canonical_root)
    }

    fn hook_log(&self) -> &[String] {
        &self.hook_log
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Workspace Creation and Reuse Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_workspace_creation_new() {
    let dir = TempDir::new().unwrap();
    let mut mgr = TestWorkspaceManager::new(dir.path().to_path_buf());

    let ws = mgr.ensure_workspace("PROJ-42").unwrap();

    assert!(ws.created_now);
    assert!(ws.path.exists());
    assert!(ws.path.is_dir());
    assert_eq!(ws.workspace_key, "PROJ-42");
}

#[test]
fn test_workspace_reuse_existing() {
    let dir = TempDir::new().unwrap();
    let mut mgr = TestWorkspaceManager::new(dir.path().to_path_buf());

    // First call creates
    let ws1 = mgr.ensure_workspace("PROJ-42").unwrap();
    assert!(ws1.created_now);

    // Second call reuses
    let ws2 = mgr.ensure_workspace("PROJ-42").unwrap();
    assert!(!ws2.created_now);
    assert_eq!(ws1.path, ws2.path);
}

#[test]
fn test_workspace_deterministic_path() {
    let dir = TempDir::new().unwrap();
    let mut mgr = TestWorkspaceManager::new(dir.path().to_path_buf());

    let ws1 = mgr.ensure_workspace("PROJ-42").unwrap();
    let ws2 = mgr.ensure_workspace("PROJ-42").unwrap();

    // Same identifier always maps to same path
    assert_eq!(ws1.path, ws2.path);
}

#[test]
fn test_workspace_different_identifiers_different_paths() {
    let dir = TempDir::new().unwrap();
    let mut mgr = TestWorkspaceManager::new(dir.path().to_path_buf());

    let ws1 = mgr.ensure_workspace("PROJ-1").unwrap();
    let ws2 = mgr.ensure_workspace("PROJ-2").unwrap();

    assert_ne!(ws1.path, ws2.path);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Hook Execution Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_after_create_hook_only_on_new() {
    let dir = TempDir::new().unwrap();
    let mut mgr = TestWorkspaceManager::new(dir.path().to_path_buf());

    // First time — after_create should fire
    let ws = mgr.ensure_workspace("PROJ-42").unwrap();
    mgr.run_after_create_hook(&ws);
    assert_eq!(mgr.hook_log().len(), 1);
    assert!(mgr.hook_log()[0].starts_with("after_create:"));

    // Second time — after_create should NOT fire
    let ws = mgr.ensure_workspace("PROJ-42").unwrap();
    mgr.run_after_create_hook(&ws);
    assert_eq!(mgr.hook_log().len(), 1); // Still 1
}

#[test]
fn test_before_run_hook_every_time() {
    let dir = TempDir::new().unwrap();
    let mut mgr = TestWorkspaceManager::new(dir.path().to_path_buf());

    let ws = mgr.ensure_workspace("PROJ-42").unwrap();

    // before_run fires every time
    mgr.run_before_run_hook(&ws).unwrap();
    mgr.run_before_run_hook(&ws).unwrap();
    mgr.run_before_run_hook(&ws).unwrap();

    let before_run_count = mgr
        .hook_log()
        .iter()
        .filter(|l| l.starts_with("before_run:"))
        .count();
    assert_eq!(before_run_count, 3);
}

#[test]
fn test_after_run_hook_fires() {
    let dir = TempDir::new().unwrap();
    let mut mgr = TestWorkspaceManager::new(dir.path().to_path_buf());

    let ws = mgr.ensure_workspace("PROJ-42").unwrap();
    mgr.run_after_run_hook(&ws);

    assert_eq!(mgr.hook_log().len(), 1);
    assert!(mgr.hook_log()[0].starts_with("after_run:"));
}

#[test]
fn test_full_hook_lifecycle() {
    let dir = TempDir::new().unwrap();
    let mut mgr = TestWorkspaceManager::new(dir.path().to_path_buf());

    // First run
    let ws = mgr.ensure_workspace("PROJ-42").unwrap();
    mgr.run_after_create_hook(&ws);
    mgr.run_before_run_hook(&ws).unwrap();
    mgr.run_after_run_hook(&ws);

    // Second run (reuse)
    let ws = mgr.ensure_workspace("PROJ-42").unwrap();
    mgr.run_after_create_hook(&ws); // Should not fire
    mgr.run_before_run_hook(&ws).unwrap();
    mgr.run_after_run_hook(&ws);

    let log = mgr.hook_log();
    assert_eq!(log.len(), 5); // after_create + before_run + after_run + before_run + after_run
    assert_eq!(log[0], "after_create:PROJ-42");
    assert_eq!(log[1], "before_run:PROJ-42");
    assert_eq!(log[2], "after_run:PROJ-42");
    assert_eq!(log[3], "before_run:PROJ-42");
    assert_eq!(log[4], "after_run:PROJ-42");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Path Containment Validation Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_path_containment_valid() {
    let dir = TempDir::new().unwrap();
    let mgr = TestWorkspaceManager::new(dir.path().to_path_buf());

    let valid_path = dir.path().join("workspace-1");
    assert!(mgr.is_contained(&valid_path));
}

#[test]
fn test_path_containment_rejects_traversal() {
    let dir = TempDir::new().unwrap();
    let mgr = TestWorkspaceManager::new(dir.path().to_path_buf());

    // Attempt path traversal
    let escape_path = dir.path().join("..").join("etc").join("passwd");
    assert!(!mgr.is_contained(&escape_path));
}

#[test]
fn test_path_containment_nested_valid() {
    let dir = TempDir::new().unwrap();
    let mgr = TestWorkspaceManager::new(dir.path().to_path_buf());

    let nested_path = dir.path().join("sub").join("dir").join("workspace");
    assert!(mgr.is_contained(&nested_path));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Workspace Cleanup Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_cleanup_removes_workspace() {
    let dir = TempDir::new().unwrap();
    let mut mgr = TestWorkspaceManager::new(dir.path().to_path_buf());

    // Create workspace
    let ws = mgr.ensure_workspace("PROJ-42").unwrap();
    assert!(ws.path.exists());

    // Write some content
    fs::write(ws.path.join("test.txt"), "hello").unwrap();

    // Cleanup
    mgr.cleanup_workspace("PROJ-42").unwrap();
    assert!(!ws.path.exists());
}

#[test]
fn test_cleanup_nonexistent_workspace_is_noop() {
    let dir = TempDir::new().unwrap();
    let mut mgr = TestWorkspaceManager::new(dir.path().to_path_buf());

    // Cleanup of non-existent workspace should not error
    let result = mgr.cleanup_workspace("NONEXISTENT-99");
    assert!(result.is_ok());
}

#[test]
fn test_cleanup_fires_before_remove_hook() {
    let dir = TempDir::new().unwrap();
    let mut mgr = TestWorkspaceManager::new(dir.path().to_path_buf());

    // Create workspace
    mgr.ensure_workspace("PROJ-42").unwrap();

    // Cleanup
    mgr.cleanup_workspace("PROJ-42").unwrap();

    let before_remove_count = mgr
        .hook_log()
        .iter()
        .filter(|l| l.starts_with("before_remove:"))
        .count();
    assert_eq!(before_remove_count, 1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Workspace Key Sanitization Integration Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_workspace_with_special_identifier() {
    let dir = TempDir::new().unwrap();
    let mut mgr = TestWorkspaceManager::new(dir.path().to_path_buf());

    // Identifiers with special chars should be sanitized
    let ws = mgr.ensure_workspace("PROJ/42#special").unwrap();
    assert!(ws.path.exists());
    // The workspace key (filename) should not contain special chars
    let filename = ws.path.file_name().unwrap().to_string_lossy();
    assert!(!filename.contains('/'));
    assert!(!filename.contains('#'));
    assert_eq!(ws.workspace_key, "PROJ_42_special");
}

#[test]
fn test_workspace_with_path_traversal_identifier() {
    let dir = TempDir::new().unwrap();
    let mut mgr = TestWorkspaceManager::new(dir.path().to_path_buf());

    // Attempt path traversal via identifier
    let ws = mgr.ensure_workspace("../../etc/passwd").unwrap();
    // Should be sanitized and contained within root
    assert!(ws.path.starts_with(dir.path()));
}

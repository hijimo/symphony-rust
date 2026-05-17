//! Unit tests for workspace sanitization and lifecycle management.
//!
//! Tests cover:
//! - sanitize_workspace_key: valid chars preserved, invalid chars replaced
//! - Dots preserved (SPEC requirement)
//! - No underscore merging in workspace module (unlike service_config)
//! - Path containment validation (no symlink escape)
//! - Workspace lifecycle hooks (after_create, before_run, after_run, before_remove)
//! - Workspace creation and reuse

use std::path::PathBuf;

use tempfile::TempDir;

use symphony_platform::config::service_config::HooksConfig;
use symphony_platform::workspace::{sanitize_workspace_key, WorkspaceError, WorkspaceManager};

// ═══════════════════════════════════════════════════════════════════════════════
// sanitize_workspace_key Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod sanitize_key {
    use super::*;

    #[test]
    fn test_sanitize_preserves_ascii_alphanumeric() {
        let result = sanitize_workspace_key("ABC123xyz").unwrap();
        assert_eq!(result, "ABC123xyz");
    }

    #[test]
    fn test_sanitize_preserves_dots() {
        // SPEC requirement: dots are valid in workspace keys
        let result = sanitize_workspace_key("my.issue.42").unwrap();
        assert_eq!(result, "my.issue.42");
    }

    #[test]
    fn test_sanitize_preserves_hyphens() {
        let result = sanitize_workspace_key("PROJ-42").unwrap();
        assert_eq!(result, "PROJ-42");
    }

    #[test]
    fn test_sanitize_preserves_underscores() {
        let result = sanitize_workspace_key("my_issue_42").unwrap();
        assert_eq!(result, "my_issue_42");
    }

    #[test]
    fn test_sanitize_replaces_spaces_with_underscore() {
        let result = sanitize_workspace_key("hello world").unwrap();
        assert_eq!(result, "hello_world");
    }

    #[test]
    fn test_sanitize_replaces_slashes_with_underscore() {
        let result = sanitize_workspace_key("foo/bar/baz").unwrap();
        assert_eq!(result, "foo_bar_baz");
    }

    #[test]
    fn test_sanitize_replaces_special_chars_with_underscore() {
        let result = sanitize_workspace_key("a@b#c$d").unwrap();
        assert_eq!(result, "a_b_c_d");
    }

    #[test]
    fn test_sanitize_no_underscore_merging() {
        // The workspace module does NOT collapse consecutive underscores
        // (unlike the service_config version which does)
        let result = sanitize_workspace_key("a///b").unwrap();
        // Each '/' becomes '_', no merging
        assert_eq!(result, "a___b");
    }

    #[test]
    fn test_sanitize_no_leading_trailing_trimming() {
        // The workspace module does NOT trim leading/trailing underscores
        let result = sanitize_workspace_key("/hello/").unwrap();
        assert_eq!(result, "_hello_");
    }

    #[test]
    fn test_sanitize_rejects_empty_string() {
        let result = sanitize_workspace_key("");
        assert!(result.is_err());
        if let Err(WorkspaceError::UnsafeIdentifier { identifier }) = result {
            assert_eq!(identifier, "");
        }
    }

    #[test]
    fn test_sanitize_rejects_single_dot() {
        let result = sanitize_workspace_key(".");
        assert!(result.is_err());
    }

    #[test]
    fn test_sanitize_rejects_double_dot() {
        let result = sanitize_workspace_key("..");
        assert!(result.is_err());
    }

    #[test]
    fn test_sanitize_rejects_all_dots() {
        let result = sanitize_workspace_key("...");
        assert!(result.is_err());
    }

    #[test]
    fn test_sanitize_slash_only_is_valid() {
        // A single slash becomes "_" which is valid
        let result = sanitize_workspace_key("/");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "_");
    }

    #[test]
    fn test_sanitize_unicode_replaced_with_underscore() {
        let result = sanitize_workspace_key("日本語").unwrap();
        assert_eq!(result, "___");
    }

    #[test]
    fn test_sanitize_mixed_valid_and_invalid() {
        let result = sanitize_workspace_key("PROJ-42: fix login").unwrap();
        assert_eq!(result, "PROJ-42__fix_login");
    }

    #[test]
    fn test_sanitize_preserves_dot_in_middle() {
        let result = sanitize_workspace_key("v1.2.3-beta").unwrap();
        assert_eq!(result, "v1.2.3-beta");
    }

    #[test]
    fn test_sanitize_complex_identifier() {
        let result = sanitize_workspace_key("feature/ABC-123_impl").unwrap();
        assert_eq!(result, "feature_ABC-123_impl");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Path Containment Validation Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod path_containment {
    use super::*;

    #[tokio::test]
    async fn test_path_inside_root_is_valid() {
        let tmp = TempDir::new().unwrap();
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());

        // Create a subdirectory inside root
        let sub = tmp.path().join("valid-workspace");
        tokio::fs::create_dir_all(&sub).await.unwrap();

        let result = mgr.validate_path_containment(&sub);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_path_outside_root_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());

        // A path that doesn't exist can't be canonicalized
        let outside = PathBuf::from("/tmp/definitely_not_inside_workspace_root_xyz");
        let result = mgr.validate_path_containment(&outside);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_symlink_escape_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());

        // Create a symlink that points outside the root
        let outside_dir = TempDir::new().unwrap();
        let link_path = tmp.path().join("escape-link");

        // Create symlink: root/escape-link -> outside_dir
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside_dir.path(), &link_path).unwrap();
            let result = mgr.validate_path_containment(&link_path);
            assert!(result.is_err());
        }
    }

    #[tokio::test]
    async fn test_nested_path_inside_root_is_valid() {
        let tmp = TempDir::new().unwrap();
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());

        let nested = tmp.path().join("level1").join("level2");
        tokio::fs::create_dir_all(&nested).await.unwrap();

        let result = mgr.validate_path_containment(&nested);
        assert!(result.is_ok());
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Workspace Lifecycle Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod workspace_lifecycle {
    use super::*;

    #[tokio::test]
    async fn test_ensure_workspace_creates_new_directory() {
        let tmp = TempDir::new().unwrap();
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());

        let ws = mgr.ensure_workspace("NEW-1").await.unwrap();
        assert!(ws.path.is_dir());
        assert!(ws.created_now);
        assert_eq!(ws.workspace_key, "NEW-1");
    }

    #[tokio::test]
    async fn test_ensure_workspace_reuses_existing_directory() {
        let tmp = TempDir::new().unwrap();
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());

        let ws1 = mgr.ensure_workspace("REUSE-1").await.unwrap();
        assert!(ws1.created_now);

        let ws2 = mgr.ensure_workspace("REUSE-1").await.unwrap();
        assert!(!ws2.created_now);
        assert_eq!(ws1.path, ws2.path);
    }

    #[tokio::test]
    async fn test_ensure_workspace_rejects_unsafe_identifier() {
        let tmp = TempDir::new().unwrap();
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());

        let result = mgr.ensure_workspace("..").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_remove_workspace_deletes_directory() {
        let tmp = TempDir::new().unwrap();
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());

        let ws = mgr.ensure_workspace("REMOVE-1").await.unwrap();
        assert!(ws.path.is_dir());

        mgr.remove_workspace("REMOVE-1").await;
        assert!(!ws.path.is_dir());
    }

    #[tokio::test]
    async fn test_remove_nonexistent_workspace_is_noop() {
        let tmp = TempDir::new().unwrap();
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());

        // Should not panic or error
        mgr.remove_workspace("NONEXISTENT-1").await;
    }

    #[tokio::test]
    async fn test_workspace_path_is_under_root() {
        let tmp = TempDir::new().unwrap();
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());

        let ws = mgr.ensure_workspace("PATH-1").await.unwrap();
        assert!(ws.path.starts_with(tmp.path()));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Workspace Hook Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod workspace_hooks {
    use super::*;

    #[tokio::test]
    async fn test_after_create_hook_runs_on_new_workspace() {
        let tmp = TempDir::new().unwrap();
        let hooks = HooksConfig {
            after_create: Some("touch .hook_ran".to_string()),
            before_run: None,
            after_run: None,
            before_remove: None,
            timeout_ms: 5_000,
        };
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), hooks);

        let ws = mgr.ensure_workspace("HOOK-CREATE-1").await.unwrap();
        assert!(ws.created_now);
        // The hook should have created a .hook_ran file
        assert!(ws.path.join(".hook_ran").exists());
    }

    #[tokio::test]
    async fn test_after_create_hook_not_run_on_existing_workspace() {
        let tmp = TempDir::new().unwrap();
        let hooks = HooksConfig {
            after_create: Some("touch .hook_ran_again".to_string()),
            before_run: None,
            after_run: None,
            before_remove: None,
            timeout_ms: 5_000,
        };
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), hooks);

        // First call creates and runs hook
        let ws1 = mgr.ensure_workspace("HOOK-SKIP-1").await.unwrap();
        assert!(ws1.created_now);

        // Remove the marker file
        tokio::fs::remove_file(ws1.path.join(".hook_ran_again"))
            .await
            .unwrap();

        // Second call should NOT run the hook
        let ws2 = mgr.ensure_workspace("HOOK-SKIP-1").await.unwrap();
        assert!(!ws2.created_now);
        assert!(!ws2.path.join(".hook_ran_again").exists());
    }

    #[tokio::test]
    async fn test_after_create_hook_failure_removes_workspace() {
        let tmp = TempDir::new().unwrap();
        let hooks = HooksConfig {
            after_create: Some("exit 1".to_string()),
            before_run: None,
            after_run: None,
            before_remove: None,
            timeout_ms: 5_000,
        };
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), hooks);

        let result = mgr.ensure_workspace("HOOK-FAIL-1").await;
        assert!(result.is_err());
        // The workspace directory should have been cleaned up
        assert!(!tmp.path().join("HOOK-FAIL-1").is_dir());
    }

    #[tokio::test]
    async fn test_before_run_hook_success() {
        let tmp = TempDir::new().unwrap();
        let hooks = HooksConfig {
            after_create: None,
            before_run: Some("touch .before_run_marker".to_string()),
            after_run: None,
            before_remove: None,
            timeout_ms: 5_000,
        };
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), hooks);

        let ws = mgr.ensure_workspace("HOOK-BEFORE-1").await.unwrap();
        mgr.run_before_run(&ws.path).await.unwrap();
        assert!(ws.path.join(".before_run_marker").exists());
    }

    #[tokio::test]
    async fn test_before_run_hook_failure_is_fatal() {
        let tmp = TempDir::new().unwrap();
        let hooks = HooksConfig {
            after_create: None,
            before_run: Some("exit 42".to_string()),
            after_run: None,
            before_remove: None,
            timeout_ms: 5_000,
        };
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), hooks);

        let ws = mgr.ensure_workspace("HOOK-BEFORE-FAIL-1").await.unwrap();
        let result = mgr.run_before_run(&ws.path).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_after_run_hook_failure_is_ignored() {
        let tmp = TempDir::new().unwrap();
        let hooks = HooksConfig {
            after_create: None,
            before_run: None,
            after_run: Some("exit 1".to_string()),
            before_remove: None,
            timeout_ms: 5_000,
        };
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), hooks);

        let ws = mgr.ensure_workspace("HOOK-AFTER-1").await.unwrap();
        // after_run should not panic or propagate error
        mgr.run_after_run(&ws.path).await;
    }

    #[tokio::test]
    async fn test_hook_timeout() {
        let tmp = TempDir::new().unwrap();
        let hooks = HooksConfig {
            after_create: None,
            before_run: None,
            after_run: None,
            before_remove: None,
            timeout_ms: 100, // Very short timeout
        };
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), hooks);

        let ws = mgr.ensure_workspace("HOOK-TIMEOUT-1").await.unwrap();
        // Run a hook that sleeps longer than the timeout
        let result = mgr.run_hook("test_hook", "sleep 10", &ws.path).await;
        assert!(matches!(result, Err(WorkspaceError::HookTimeout { .. })));
    }

    #[tokio::test]
    async fn test_no_hooks_configured_is_noop() {
        let tmp = TempDir::new().unwrap();
        let hooks = HooksConfig {
            after_create: None,
            before_run: None,
            after_run: None,
            before_remove: None,
            timeout_ms: 5_000,
        };
        let mgr = WorkspaceManager::new(tmp.path().to_path_buf(), hooks);

        let ws = mgr.ensure_workspace("NO-HOOKS-1").await.unwrap();
        // These should all succeed silently
        mgr.run_before_run(&ws.path).await.unwrap();
        mgr.run_after_run(&ws.path).await;
    }
}

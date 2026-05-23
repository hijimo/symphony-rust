//! Unit tests for config and workflow modules.
//!
//! Tests cover:
//! - workflow_loader: front matter parsing, empty file, non-map YAML, no front matter, missing file
//! - service_config: default values, $VAR resolution, ~ expansion, relative path resolution, validation
//! - sanitize_workspace_key: special chars, "..", ".", empty string, valid identifiers

use std::collections::HashMap;
use std::path::Path;

use tempfile::TempDir;

use symphony_platform::config::service_config::{
    resolve_path, resolve_value, sanitize_workspace_key, CodexConfig, HooksConfig, ServiceConfig,
    ServiceConfigError, TrackerKind,
};
use symphony_platform::config::{
    load_workflow, parse_workflow, WorkflowDefinition, WorkflowLoadError,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Workflow Loader Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod workflow_loader {
    use super::*;

    #[test]
    fn test_front_matter_parsing_basic() {
        let content = "---\ntracker:\n  kind: linear\n  project_slug: my-project\npolling:\n  interval_ms: 5000\n---\nYou are working on {{ issue.title }}.\n";

        let result = parse_workflow(content).unwrap();

        assert!(result.config.contains_key("tracker"));
        assert!(result.config.contains_key("polling"));
        assert_eq!(
            result.prompt_template,
            "You are working on {{ issue.title }}."
        );
    }

    #[test]
    fn test_front_matter_with_nested_config() {
        let content = "---\ntracker:\n  kind: linear\n  api_key: $LINEAR_API_KEY\n  project_slug: eng\nagent:\n  max_concurrent: 5\n  max_turns: 20\n---\nWork on {{ issue.identifier }}: {{ issue.title }}\n";

        let result = parse_workflow(content).unwrap();

        assert!(result.config.contains_key("tracker"));
        assert!(result.config.contains_key("agent"));
        assert!(result.prompt_template.contains("{{ issue.identifier }}"));
    }

    #[test]
    fn test_empty_file() {
        let content = "";
        let result = parse_workflow(content).unwrap();

        assert!(result.config.is_empty());
        assert_eq!(result.prompt_template, "");
    }

    #[test]
    fn test_non_map_yaml_list() {
        let content = "---\n- item1\n- item2\n- item3\n---\nPrompt body.\n";

        let result = parse_workflow(content);
        assert!(matches!(
            result,
            Err(WorkflowLoadError::WorkflowFrontMatterNotAMap)
        ));
    }

    #[test]
    fn test_non_map_yaml_scalar() {
        let content = "---\njust a plain string\n---\nPrompt body.\n";

        let result = parse_workflow(content);
        assert!(matches!(
            result,
            Err(WorkflowLoadError::WorkflowFrontMatterNotAMap)
        ));
    }

    #[test]
    fn test_non_map_yaml_number() {
        let content = "---\n42\n---\nPrompt body.\n";

        let result = parse_workflow(content);
        assert!(matches!(
            result,
            Err(WorkflowLoadError::WorkflowFrontMatterNotAMap)
        ));
    }

    #[test]
    fn test_no_front_matter() {
        let content =
            "This is just a prompt template with no YAML config.\n\nIt has multiple lines.\n";

        let result = parse_workflow(content).unwrap();

        assert!(result.config.is_empty());
        assert!(result.prompt_template.contains("just a prompt template"));
    }

    #[test]
    fn test_missing_file() {
        let result = load_workflow(Path::new("/nonexistent/path/WORKFLOW.md"));
        assert!(matches!(
            result,
            Err(WorkflowLoadError::MissingWorkflowFile { .. })
        ));
    }

    #[test]
    fn test_load_workflow_from_disk() {
        let dir = TempDir::new().unwrap();
        let workflow_path = dir.path().join("WORKFLOW.md");
        std::fs::write(
            &workflow_path,
            "---\ntracker:\n  kind: linear\n---\nHello {{ issue.title }}\n",
        )
        .unwrap();

        let result = load_workflow(&workflow_path).unwrap();
        assert!(result.config.contains_key("tracker"));
        assert_eq!(result.prompt_template, "Hello {{ issue.title }}");
    }

    #[test]
    fn test_empty_front_matter() {
        let content = "---\n---\nPrompt body here.\n";

        let result = parse_workflow(content).unwrap();
        assert!(result.config.is_empty());
        assert_eq!(result.prompt_template, "Prompt body here.");
    }

    #[test]
    fn test_front_matter_with_comments_only() {
        let content = "---\n# This is a comment\n# Another comment\n---\nPrompt body.\n";

        let result = parse_workflow(content).unwrap();
        // YAML comments parse as Null, which we treat as empty map
        assert!(result.config.is_empty());
        assert_eq!(result.prompt_template, "Prompt body.");
    }

    #[test]
    fn test_invalid_yaml_syntax() {
        let content = "---\n: invalid: yaml: [[\n---\nPrompt body.\n";

        let result = parse_workflow(content);
        assert!(matches!(
            result,
            Err(WorkflowLoadError::WorkflowParseError { .. })
        ));
    }

    #[test]
    fn test_no_closing_delimiter() {
        // If there's no closing ---, treat entire content as prompt
        let content = "---\nthis looks like front matter but has no closing\n";

        let result = parse_workflow(content).unwrap();
        assert!(result.config.is_empty());
        // The entire content becomes the prompt template
        assert!(result.prompt_template.contains("---"));
    }

    #[test]
    fn test_multiline_prompt_body() {
        let content = "---\nkey: value\n---\nLine 1.\n\nLine 2.\n\nLine 3.\n";

        let result = parse_workflow(content).unwrap();
        assert_eq!(result.prompt_template, "Line 1.\n\nLine 2.\n\nLine 3.");
    }

    #[test]
    fn test_front_matter_preserves_types() {
        let content = "---\ncount: 42\nenabled: true\nname: symphony\n---\nPrompt.\n";

        let result = parse_workflow(content).unwrap();
        assert!(result.config.contains_key("count"));
        assert!(result.config.contains_key("enabled"));
        assert!(result.config.contains_key("name"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ServiceConfig Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod service_config_tests {
    use super::*;

    #[test]
    fn test_default_values() {
        let config = ServiceConfig::default();

        assert_eq!(config.tracker_kind, TrackerKind::Linear);
        assert_eq!(config.tracker_endpoint, "https://api.linear.app/graphql");
        assert_eq!(config.poll_interval_ms, 30_000);
        assert_eq!(config.max_concurrent_agents, 10);
        assert_eq!(config.max_turns, 20);
        assert_eq!(config.max_retry_backoff_ms, 300_000);
        assert!(config.active_states.contains(&"Todo".to_string()));
        assert!(config.terminal_states.contains(&"Done".to_string()));
    }

    #[test]
    fn test_default_hooks_config() {
        let hooks = HooksConfig::default();

        assert!(hooks.after_create.is_none());
        assert!(hooks.before_run.is_none());
        assert!(hooks.after_run.is_none());
        assert!(hooks.before_remove.is_none());
        assert_eq!(hooks.timeout_ms, 60_000);
    }

    #[test]
    fn test_default_codex_config() {
        let codex = CodexConfig::default();

        assert_eq!(codex.command, "codex app-server");
        assert!(codex.approval_policy.is_none());
        assert_eq!(codex.stall_timeout_ms, 300_000);
    }

    #[test]
    fn test_resolve_value_literal() {
        let result = resolve_value("hello-world").unwrap();
        assert_eq!(result, "hello-world");
    }

    #[test]
    fn test_resolve_value_env_var_success() {
        std::env::set_var("SYMPHONY_TEST_VAR_1", "resolved_value");
        let result = resolve_value("$SYMPHONY_TEST_VAR_1").unwrap();
        assert_eq!(result, "resolved_value");
        std::env::remove_var("SYMPHONY_TEST_VAR_1");
    }

    #[test]
    fn test_resolve_value_env_var_missing() {
        std::env::remove_var("SYMPHONY_NONEXISTENT_VAR");
        let result = resolve_value("$SYMPHONY_NONEXISTENT_VAR");
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_path_tilde_expansion() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let result = resolve_path("~/workspaces").unwrap();
        assert!(result.starts_with(&home));
        assert!(result.to_string_lossy().ends_with("workspaces"));
    }

    #[test]
    fn test_resolve_path_absolute() {
        let result = resolve_path("/var/symphony/workspaces").unwrap();
        assert_eq!(result, std::path::PathBuf::from("/var/symphony/workspaces"));
    }

    #[test]
    fn test_resolve_path_relative() {
        let result = resolve_path("relative/path").unwrap();
        assert!(result.is_absolute());
        assert!(result.to_string_lossy().ends_with("relative/path"));
    }

    #[test]
    fn test_resolve_path_env_var() {
        std::env::set_var("SYMPHONY_TEST_PATH", "/custom/workspace");
        let result = resolve_path("$SYMPHONY_TEST_PATH").unwrap();
        assert_eq!(result, std::path::PathBuf::from("/custom/workspace"));
        std::env::remove_var("SYMPHONY_TEST_PATH");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// sanitize_workspace_key Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod sanitize_workspace_key_tests {
    use super::*;

    #[test]
    fn test_special_chars_replaced() {
        // SPEC: [^a-zA-Z0-9._-] → '_', no merging, no trimming
        assert_eq!(sanitize_workspace_key("hello world!"), "hello_world_");
        assert_eq!(sanitize_workspace_key("foo/bar/baz"), "foo_bar_baz");
        assert_eq!(sanitize_workspace_key("a@b#c$d"), "a_b_c_d");
        assert_eq!(sanitize_workspace_key("issue (1)"), "issue__1_");
    }

    #[test]
    fn test_dotdot_path_traversal_prevented() {
        // ".." is rejected as unsafe → returns "_default"
        let result = sanitize_workspace_key("..");
        assert_eq!(result, "_default");
    }

    #[test]
    fn test_dotdot_in_path() {
        // "../etc/passwd" → ".._etc_passwd" (dots preserved, slashes replaced)
        let result = sanitize_workspace_key("../etc/passwd");
        assert_eq!(result, ".._etc_passwd");
    }

    #[test]
    fn test_single_dot() {
        let result = sanitize_workspace_key(".");
        assert_eq!(result, "_default");
    }

    #[test]
    fn test_dot_prefix() {
        // ".hidden" → ".hidden" (dots are preserved per SPEC)
        let result = sanitize_workspace_key(".hidden");
        assert_eq!(result, ".hidden");
    }

    #[test]
    fn test_empty_string() {
        assert_eq!(sanitize_workspace_key(""), "_default");
    }

    #[test]
    fn test_valid_identifiers_unchanged() {
        assert_eq!(sanitize_workspace_key("PROJ-42"), "PROJ-42");
        assert_eq!(sanitize_workspace_key("feature_branch"), "feature_branch");
        assert_eq!(sanitize_workspace_key("issue-123"), "issue-123");
        assert_eq!(sanitize_workspace_key("ABC-123"), "ABC-123");
    }

    #[test]
    fn test_consecutive_special_chars_no_collapse() {
        // SPEC: no merging of consecutive underscores
        assert_eq!(sanitize_workspace_key("a///b"), "a___b");
        assert_eq!(sanitize_workspace_key("a   b"), "a___b");
        assert_eq!(sanitize_workspace_key("a@#$b"), "a___b");
    }

    #[test]
    fn test_leading_trailing_special_chars_not_stripped() {
        // SPEC: no trimming
        assert_eq!(sanitize_workspace_key("___hello___"), "___hello___");
        assert_eq!(sanitize_workspace_key("///hello///"), "___hello___");
        assert_eq!(sanitize_workspace_key("@hello@"), "_hello_");
    }

    #[test]
    fn test_unicode_chars_replaced() {
        // Non-ASCII alphanumeric chars should be replaced
        assert_eq!(sanitize_workspace_key("héllo"), "h_llo");
        // All-underscore result from unicode is valid (not all-dots)
        assert_eq!(sanitize_workspace_key("日本語"), "___");
    }

    #[test]
    fn test_all_special_chars_returns_result() {
        // "@#$%^&*" → all underscores, which is valid
        assert_eq!(sanitize_workspace_key("@#$%^&*"), "_______");
        // "..." is all-dots → unsafe → _default
        assert_eq!(sanitize_workspace_key("..."), "_default");
    }

    #[test]
    fn test_hyphen_and_underscore_preserved() {
        assert_eq!(sanitize_workspace_key("my-issue_42"), "my-issue_42");
        assert_eq!(sanitize_workspace_key("a-b_c-d"), "a-b_c-d");
    }
}

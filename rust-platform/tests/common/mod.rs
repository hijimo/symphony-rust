//! Shared test infrastructure for Symphony platform adapter tests.
//!
//! Provides mock implementations, test helpers, and fixtures used across
//! unit and integration tests.

#![allow(dead_code)]

pub mod mock_tracker;
pub mod mock_codex;
pub mod git_host;
pub mod github_host;
pub mod gitlab_host;

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use tempfile::TempDir;

use symphony_platform::config::{Config, PlatformConfig, PollingConfig, WorkflowConfig};
use symphony_platform::platform::{Issue, IssueId};

// ─── Test Issue Helpers ───────────────────────────────────────────────────────

/// Create a test issue with configurable fields.
pub fn create_test_issue(id: u64, identifier: &str, title: &str) -> Issue {
    Issue {
        id: IssueId(id),
        number: id,
        title: title.to_string(),
        description: Some(format!("Description for {}", identifier)),
        url: format!("https://github.com/test-org/test-repo/issues/{}", id),
        assignee: None,
        workflow_state: Some("workflow::todo".to_string()),
        branch_name: format!("symphony/{}", identifier.to_lowercase()),
        priority: None,
        labels: vec!["workflow::todo".to_string()],
        blocked_by: Vec::new(),
        created_at: Some(Utc::now()),
        updated_at: Some(Utc::now()),
    }
}

/// Create a test issue with specific priority.
pub fn create_test_issue_with_priority(
    id: u64,
    identifier: &str,
    priority: Option<u8>,
    created_at: DateTime<Utc>,
) -> Issue {
    Issue {
        id: IssueId(id),
        number: id,
        title: format!("Issue {}", identifier),
        description: None,
        url: format!("https://github.com/test-org/test-repo/issues/{}", id),
        assignee: None,
        workflow_state: Some("workflow::todo".to_string()),
        branch_name: format!("symphony/{}", identifier.to_lowercase()),
        priority,
        labels: vec!["workflow::todo".to_string()],
        blocked_by: Vec::new(),
        created_at: Some(created_at),
        updated_at: Some(Utc::now()),
    }
}

/// Create a test issue in a specific state.
pub fn create_test_issue_in_state(id: u64, identifier: &str, state: &str) -> Issue {
    Issue {
        id: IssueId(id),
        number: id,
        title: format!("Issue {}", identifier),
        description: None,
        url: format!("https://github.com/test-org/test-repo/issues/{}", id),
        assignee: None,
        workflow_state: Some(state.to_string()),
        branch_name: format!("symphony/{}", identifier.to_lowercase()),
        priority: None,
        labels: vec![state.to_string()],
        blocked_by: Vec::new(),
        created_at: Some(Utc::now()),
        updated_at: Some(Utc::now()),
    }
}

/// Create a test issue with blockers.
pub fn create_test_issue_with_blockers(id: u64, identifier: &str, blockers: Vec<IssueId>) -> Issue {
    Issue {
        id: IssueId(id),
        number: id,
        title: format!("Issue {}", identifier),
        description: None,
        url: format!("https://github.com/test-org/test-repo/issues/{}", id),
        assignee: None,
        workflow_state: Some("workflow::todo".to_string()),
        branch_name: format!("symphony/{}", identifier.to_lowercase()),
        priority: None,
        labels: vec!["workflow::todo".to_string()],
        blocked_by: blockers,
        created_at: Some(Utc::now()),
        updated_at: Some(Utc::now()),
    }
}

// ─── Test Config Helpers ──────────────────────────────────────────────────────

/// Create a test Config with sensible defaults.
pub fn create_test_config() -> Config {
    let mut states = HashMap::new();
    states.insert("backlog".to_string(), "workflow::backlog".to_string());
    states.insert("todo".to_string(), "workflow::todo".to_string());
    states.insert("in_progress".to_string(), "workflow::in-progress".to_string());
    states.insert("human_review".to_string(), "workflow::human-review".to_string());
    states.insert("rework".to_string(), "workflow::rework".to_string());
    states.insert("done".to_string(), "workflow::done".to_string());

    Config {
        platform: Some(PlatformConfig {
            kind: "github".to_string(),
            api_token: "$TEST_GITHUB_TOKEN".to_string(),
            base_url: "https://api.github.com".to_string(),
            owner: "test-org".to_string(),
            repo: "test-repo".to_string(),
            project_id: None,
            allow_custom_host: false,
            issue_filter: Default::default(),
            workflow: WorkflowConfig {
                states,
                active_states: vec![
                    "todo".to_string(),
                    "in_progress".to_string(),
                    "rework".to_string(),
                ],
                terminal_states: vec!["done".to_string()],
            },
        }),
        tracker: None,
        polling: PollingConfig { interval_ms: 5_000 },
    }
}

/// Create a test Config with custom active/terminal states.
pub fn create_test_config_with_states(
    active_states: Vec<String>,
    terminal_states: Vec<String>,
) -> Config {
    let mut config = create_test_config();
    if let Some(ref mut platform) = config.platform {
        platform.workflow.active_states = active_states;
        platform.workflow.terminal_states = terminal_states;
    }
    config
}

// ─── TempWorkspace Helper ─────────────────────────────────────────────────────

/// A temporary workspace that auto-cleans on drop.
pub struct TempWorkspace {
    pub dir: TempDir,
}

impl TempWorkspace {
    /// Create a new temporary workspace directory.
    pub fn new() -> Self {
        Self {
            dir: TempDir::new().expect("failed to create temp dir"),
        }
    }

    /// Create a new temporary workspace with a specific prefix.
    pub fn with_prefix(prefix: &str) -> Self {
        Self {
            dir: TempDir::with_prefix(prefix).expect("failed to create temp dir"),
        }
    }

    /// Get the path to the workspace root.
    pub fn path(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }

    /// Create a subdirectory within the workspace.
    pub fn create_subdir(&self, name: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::create_dir_all(&path).expect("failed to create subdir");
        path
    }

    /// Write a file within the workspace.
    pub fn write_file(&self, relative_path: &str, content: &str) -> PathBuf {
        let path = self.dir.path().join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dirs");
        }
        std::fs::write(&path, content).expect("failed to write file");
        path
    }
}

// ─── Mock HTTP Response Helpers ───────────────────────────────────────────────

/// Returns a mock GitHub issues API JSON response.
pub fn mock_github_issues_response() -> Value {
    json!([
        {
            "id": 1001,
            "number": 42,
            "title": "Implement user authentication",
            "body": "We need OAuth2 support for the API gateway.",
            "html_url": "https://github.com/test-org/test-repo/issues/42",
            "user": {"login": "alice"},
            "assignee": {"login": "bob"},
            "labels": [
                {"id": 1, "name": "workflow::todo", "color": "0075ca"},
                {"id": 2, "name": "priority::high", "color": "d73a4a"}
            ],
            "state": "open",
            "created_at": "2025-01-10T10:00:00Z",
            "updated_at": "2025-01-12T15:30:00Z"
        },
        {
            "id": 1002,
            "number": 43,
            "title": "Fix database connection pooling",
            "body": "Connection pool exhaustion under high load.",
            "html_url": "https://github.com/test-org/test-repo/issues/43",
            "user": {"login": "charlie"},
            "assignee": null,
            "labels": [
                {"id": 3, "name": "workflow::in-progress", "color": "0075ca"},
                {"id": 4, "name": "bug", "color": "d73a4a"}
            ],
            "state": "open",
            "created_at": "2025-01-11T08:00:00Z",
            "updated_at": "2025-01-13T09:00:00Z"
        }
    ])
}

/// Returns a mock GitHub user response for credential validation.
pub fn mock_github_user_response() -> Value {
    json!({
        "login": "symphony-bot",
        "id": 99999,
        "type": "User"
    })
}

// ─── Legacy Compatibility Helpers ─────────────────────────────────────────────
// These are used by pre-existing tests (github_adapter_test.rs, etc.)

/// Configuration struct mirroring the production PlatformConfig for test use.
#[derive(Debug, Clone)]
pub struct TestPlatformConfig {
    pub kind: String,
    pub api_token: String,
    pub base_url: String,
    pub owner: String,
    pub repo: String,
    pub project_id: Option<u64>,
    pub allow_custom_host: bool,
    pub workflow_states: HashMap<String, String>,
    pub active_states: Vec<String>,
    pub terminal_states: Vec<String>,
}

/// Create a test PlatformConfig pointing at a wiremock server.
pub fn test_config(base_url: &str) -> TestPlatformConfig {
    let mut workflow_states = HashMap::new();
    workflow_states.insert("backlog".to_string(), "workflow::backlog".to_string());
    workflow_states.insert("todo".to_string(), "workflow::todo".to_string());
    workflow_states.insert(
        "in_progress".to_string(),
        "workflow::in-progress".to_string(),
    );
    workflow_states.insert(
        "human_review".to_string(),
        "workflow::human-review".to_string(),
    );
    workflow_states.insert("rework".to_string(), "workflow::rework".to_string());
    workflow_states.insert("merging".to_string(), "workflow::merging".to_string());
    workflow_states.insert("done".to_string(), "workflow::done".to_string());

    TestPlatformConfig {
        kind: "github".to_string(),
        api_token: "test-token-12345".to_string(),
        base_url: base_url.to_string(),
        owner: "test-org".to_string(),
        repo: "test-repo".to_string(),
        project_id: None,
        allow_custom_host: true,
        workflow_states,
        active_states: vec![
            "todo".to_string(),
            "in_progress".to_string(),
            "rework".to_string(),
        ],
        terminal_states: vec!["done".to_string()],
    }
}

/// Create a test config for GitLab pointing at a wiremock server.
pub fn test_gitlab_config(base_url: &str) -> TestPlatformConfig {
    let mut config = test_config(base_url);
    config.kind = "gitlab".to_string();
    config.project_id = Some(12345);
    config
}

/// Returns a mock GitHub comments list response.
pub fn mock_comments_response() -> Value {
    json!([
        {
            "id": 5001,
            "body": "I'll take a look at this issue.",
            "user": {"login": "bob"},
            "created_at": "2025-01-10T11:00:00Z",
            "updated_at": "2025-01-10T11:00:00Z"
        },
        {
            "id": 5002,
            "body": "## Codex Workpad\n\n### Plan\n- Step 1: Analyze requirements\n- Step 2: Implement solution\n\n### Status\nIn progress",
            "user": {"login": "symphony-bot"},
            "created_at": "2025-01-10T12:00:00Z",
            "updated_at": "2025-01-12T08:00:00Z"
        },
        {
            "id": 5003,
            "body": "Looks good so far, keep going!",
            "user": {"login": "alice"},
            "created_at": "2025-01-11T09:00:00Z",
            "updated_at": "2025-01-11T09:00:00Z"
        }
    ])
}

/// Returns a mock GitHub create comment response.
pub fn mock_create_comment_response() -> Value {
    json!({
        "id": 6001,
        "body": "Test comment body",
        "user": {"login": "symphony-bot"},
        "created_at": "2025-01-14T10:00:00Z",
        "updated_at": "2025-01-14T10:00:00Z"
    })
}

/// Returns a mock GitHub add labels response.
pub fn mock_add_labels_response() -> Value {
    json!([
        {"id": 2, "name": "workflow::todo", "color": "0075ca"},
        {"id": 8, "name": "bug", "color": "d73a4a"}
    ])
}

/// Returns a mock labels list response (GitHub format).
pub fn mock_labels_response() -> Value {
    json!([
        {"id": 1, "name": "workflow::backlog", "color": "ededed"},
        {"id": 2, "name": "workflow::todo", "color": "0075ca"},
        {"id": 3, "name": "workflow::in-progress", "color": "fbca04"},
        {"id": 4, "name": "workflow::human-review", "color": "d876e3"},
        {"id": 5, "name": "workflow::rework", "color": "e4e669"},
        {"id": 6, "name": "workflow::merging", "color": "0e8a16"},
        {"id": 7, "name": "workflow::done", "color": "5319e7"},
        {"id": 8, "name": "bug", "color": "d73a4a"},
        {"id": 9, "name": "enhancement", "color": "a2eeef"},
        {"id": 10, "name": "priority::high", "color": "b60205"}
    ])
}

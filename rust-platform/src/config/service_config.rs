//! Service Configuration — typed runtime config derived from WORKFLOW.md front matter.
//!
//! Resolves $VAR environment variables, applies defaults, expands paths,
//! and validates the configuration for dispatch readiness.
//!
//! SPEC reference: Section 4.1.3, Section 6.1-6.4

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_yaml::Value as YamlValue;
use thiserror::Error;

use super::workflow_loader::WorkflowDefinition;

/// Tracker kind enumeration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackerKind {
    Linear,
    GitHub,
    GitLab,
}

/// Hooks configuration (SPEC Section 5.3.4).
#[derive(Debug, Clone)]
pub struct HooksConfig {
    pub after_create: Option<String>,
    pub before_run: Option<String>,
    pub after_run: Option<String>,
    pub before_remove: Option<String>,
    pub timeout_ms: u64,
}

/// Workspace garbage-collection configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceGcConfig {
    pub gc_interval_ms: u64,
    pub gc_retention_ms: u64,
    pub gc_batch_size: usize,
    pub gc_cycle_timeout_ms: u64,
}

impl Default for WorkspaceGcConfig {
    fn default() -> Self {
        Self {
            gc_interval_ms: 300_000,
            gc_retention_ms: 3_600_000,
            gc_batch_size: 10,
            gc_cycle_timeout_ms: 120_000,
        }
    }
}

impl Default for HooksConfig {
    fn default() -> Self {
        Self {
            after_create: None,
            before_run: None,
            after_run: None,
            before_remove: None,
            timeout_ms: 60_000,
        }
    }
}

/// Codex app-server configuration (SPEC Section 5.3.6).
#[derive(Debug, Clone)]
pub struct CodexConfig {
    pub command: String,
    /// Approval policy: can be a simple string ("never") or a complex map.
    pub approval_policy: Option<serde_json::Value>,
    pub thread_sandbox: Option<String>,
    /// Turn sandbox policy: can be a simple string or a complex map.
    pub turn_sandbox_policy: Option<serde_json::Value>,
    pub turn_timeout_ms: u64,
    pub read_timeout_ms: u64,
    pub stall_timeout_ms: i64,
}

impl Default for CodexConfig {
    fn default() -> Self {
        Self {
            command: "codex app-server".to_string(),
            approval_policy: None,
            thread_sandbox: None,
            turn_sandbox_policy: None,
            turn_timeout_ms: 3_600_000,
            read_timeout_ms: 5_000,
            stall_timeout_ms: 300_000,
        }
    }
}

/// Typed runtime configuration (SPEC Section 4.1.3 + Section 6.4).
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    // -- tracker --
    pub tracker_kind: TrackerKind,
    pub tracker_endpoint: String,
    pub tracker_api_key: String,
    pub tracker_project_slug: String,
    pub active_states: Vec<String>,
    pub terminal_states: Vec<String>,
    pub workflow_labels: Vec<String>,

    // -- polling --
    pub poll_interval_ms: u64,

    // -- workspace --
    pub workspace_root: PathBuf,
    pub workspace_gc: WorkspaceGcConfig,

    // -- hooks --
    pub hooks: HooksConfig,

    // -- agent --
    pub max_concurrent_agents: usize,
    pub max_turns: u32,
    pub max_retry_backoff_ms: u64,
    pub max_concurrent_agents_by_state: HashMap<String, usize>,
    pub blocker_check_states: Vec<String>,

    // -- codex --
    pub codex: CodexConfig,

    // -- extensions --
    pub server_port: Option<u16>,
    pub ssh_hosts: Vec<String>,
    pub max_concurrent_agents_per_host: Option<usize>,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            tracker_kind: TrackerKind::Linear,
            tracker_endpoint: "https://api.linear.app/graphql".to_string(),
            tracker_api_key: String::new(),
            tracker_project_slug: String::new(),
            active_states: vec!["Todo".to_string(), "In Progress".to_string()],
            terminal_states: vec![
                "Closed".to_string(),
                "Cancelled".to_string(),
                "Canceled".to_string(),
                "Duplicate".to_string(),
                "Done".to_string(),
            ],
            workflow_labels: default_workflow_labels(),
            poll_interval_ms: 30_000,
            workspace_root: std::env::temp_dir().join("symphony_workspaces"),
            workspace_gc: WorkspaceGcConfig::default(),
            hooks: HooksConfig::default(),
            max_concurrent_agents: 10,
            max_turns: 20,
            max_retry_backoff_ms: 300_000,
            max_concurrent_agents_by_state: HashMap::new(),
            blocker_check_states: vec!["todo".to_string()],
            codex: CodexConfig::default(),
            server_port: None,
            ssh_hosts: Vec::new(),
            max_concurrent_agents_per_host: None,
        }
    }
}

fn default_workflow_labels() -> Vec<String> {
    vec!["Backlog".to_string(), "Human Review".to_string()]
}

/// Errors from service config parsing.
#[derive(Debug, Error)]
pub enum ServiceConfigError {
    #[error("missing environment variable: ${0}")]
    MissingEnvVar(String),

    #[error("invalid configuration value: {field} — {detail}")]
    InvalidValue { field: String, detail: String },

    #[error("missing required field: {0}")]
    MissingField(String),

    #[error("unsupported tracker kind: {0}")]
    UnsupportedTrackerKind(String),

    #[error("dispatch preflight validation failed: {0}")]
    ValidationFailed(String),
}

impl ServiceConfig {
    /// Build a typed ServiceConfig from a WorkflowDefinition.
    ///
    /// `workflow_dir` is the directory containing WORKFLOW.md, used for
    /// resolving relative paths.
    pub fn from_workflow(
        workflow: &WorkflowDefinition,
        workflow_dir: &Path,
    ) -> Result<Self, ServiceConfigError> {
        let config = &workflow.config;

        // -- tracker section --
        let tracker = get_section(config, "tracker");
        let tracker_kind = parse_tracker_kind(&tracker)?;
        let tracker_endpoint = get_string_or_default(
            &tracker,
            "endpoint",
            default_endpoint_for_kind(&tracker_kind),
        );
        let tracker_api_key = resolve_tracker_api_key(&tracker, &tracker_kind)?;
        let tracker_project_slug = get_string_or_default(&tracker, "project_slug", String::new());
        let active_states = get_string_list_or_default(
            &tracker,
            "active_states",
            vec!["Todo".to_string(), "In Progress".to_string()],
        );
        let terminal_states = get_string_list_or_default(
            &tracker,
            "terminal_states",
            vec![
                "Closed".to_string(),
                "Cancelled".to_string(),
                "Canceled".to_string(),
                "Duplicate".to_string(),
                "Done".to_string(),
            ],
        );
        let workflow_labels =
            get_string_list_or_default(&tracker, "workflow_labels", default_workflow_labels());

        // -- polling section --
        let polling = get_section(config, "polling");
        let poll_interval_ms = get_u64_or_default(&polling, "interval_ms", 30_000);

        // -- workspace section --
        let workspace = get_section(config, "workspace");
        let workspace_root = resolve_workspace_root(&workspace, workflow_dir);
        let workspace_gc = parse_workspace_gc(&workspace);

        // -- hooks section --
        let hooks_section = get_section(config, "hooks");
        let hooks = parse_hooks(&hooks_section);

        // -- agent section --
        let agent = get_section(config, "agent");
        let max_concurrent_agents =
            get_u64_or_default(&agent, "max_concurrent_agents", 10) as usize;
        let max_turns = get_u64_or_default(&agent, "max_turns", 20) as u32;
        let max_retry_backoff_ms = get_u64_or_default(&agent, "max_retry_backoff_ms", 300_000);
        let max_concurrent_agents_by_state = parse_state_concurrency_map(&agent);
        let blocker_check_states =
            get_string_list_or_default(&agent, "blocker_check_states", vec!["todo".to_string()]);

        // -- codex section --
        let codex_section = get_section(config, "codex");
        let codex = parse_codex_config(&codex_section);

        // -- server extension --
        let server = get_section(config, "server");
        let server_port = get_optional_u16(&server, "port");

        // -- worker/SSH extension --
        let worker = get_section(config, "worker");
        let ssh_hosts = get_string_list_or_default(&worker, "ssh_hosts", Vec::new());
        let max_concurrent_agents_per_host =
            worker
                .get("max_concurrent_agents_per_host")
                .and_then(|v| match v {
                    YamlValue::Number(n) => n.as_u64().map(|n| n as usize),
                    _ => None,
                });

        Ok(ServiceConfig {
            tracker_kind,
            tracker_endpoint,
            tracker_api_key,
            tracker_project_slug,
            active_states,
            terminal_states,
            workflow_labels,
            poll_interval_ms,
            workspace_root,
            workspace_gc,
            hooks,
            max_concurrent_agents,
            max_turns,
            max_retry_backoff_ms,
            max_concurrent_agents_by_state,
            blocker_check_states,
            codex,
            server_port,
            ssh_hosts,
            max_concurrent_agents_per_host,
        })
    }

    /// Dispatch preflight validation (SPEC Section 6.3).
    ///
    /// Validates that the config has enough information to poll and launch workers.
    pub fn validate_for_dispatch(&self) -> Result<(), ServiceConfigError> {
        // tracker.api_key must be non-empty after resolution
        if self.tracker_api_key.is_empty() {
            return Err(ServiceConfigError::ValidationFailed(
                "tracker.api_key is empty after resolution".to_string(),
            ));
        }

        // tracker.project_slug must be present for Linear
        if self.tracker_kind == TrackerKind::Linear && self.tracker_project_slug.is_empty() {
            return Err(ServiceConfigError::ValidationFailed(
                "tracker.project_slug is required for Linear tracker".to_string(),
            ));
        }

        // codex.command must be non-empty
        if self.codex.command.is_empty() {
            return Err(ServiceConfigError::ValidationFailed(
                "codex.command is empty".to_string(),
            ));
        }

        // max_turns must be positive
        if self.max_turns == 0 {
            return Err(ServiceConfigError::InvalidValue {
                field: "agent.max_turns".to_string(),
                detail: "must be a positive integer (> 0)".to_string(),
            });
        }

        // hooks.timeout_ms must be positive if specified
        if self.hooks.timeout_ms == 0 {
            return Err(ServiceConfigError::InvalidValue {
                field: "hooks.timeout_ms".to_string(),
                detail: "must be > 0".to_string(),
            });
        }

        if self.workspace_gc.gc_batch_size == 0 {
            return Err(ServiceConfigError::InvalidValue {
                field: "workspace.gc_batch_size".to_string(),
                detail: "must be > 0".to_string(),
            });
        }

        if self.workspace_gc.gc_cycle_timeout_ms == 0 {
            return Err(ServiceConfigError::InvalidValue {
                field: "workspace.gc_cycle_timeout_ms".to_string(),
                detail: "must be > 0".to_string(),
            });
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Extract a section from the config map as a sub-map.
fn get_section(config: &HashMap<String, YamlValue>, key: &str) -> HashMap<String, YamlValue> {
    config
        .get(key)
        .and_then(|v| match v {
            YamlValue::Mapping(m) => {
                let mut map = HashMap::new();
                for (k, val) in m {
                    if let YamlValue::String(ks) = k {
                        map.insert(ks.clone(), val.clone());
                    }
                }
                Some(map)
            }
            _ => None,
        })
        .unwrap_or_default()
}

/// Get a string value from a section, with a default fallback.
fn get_string_or_default(
    section: &HashMap<String, YamlValue>,
    key: &str,
    default: String,
) -> String {
    section
        .get(key)
        .and_then(|v| match v {
            YamlValue::String(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or(default)
}

/// Get a u64 value from a section, with a default fallback.
fn get_u64_or_default(section: &HashMap<String, YamlValue>, key: &str, default: u64) -> u64 {
    section
        .get(key)
        .and_then(|v| match v {
            YamlValue::Number(n) => n.as_u64(),
            _ => None,
        })
        .unwrap_or(default)
}

/// Get an i64 value from a section, with a default fallback.
fn get_i64_or_default(section: &HashMap<String, YamlValue>, key: &str, default: i64) -> i64 {
    section
        .get(key)
        .and_then(|v| match v {
            YamlValue::Number(n) => n.as_i64(),
            _ => None,
        })
        .unwrap_or(default)
}

/// Get an optional u16 value from a section.
fn get_optional_u16(section: &HashMap<String, YamlValue>, key: &str) -> Option<u16> {
    section.get(key).and_then(|v| match v {
        YamlValue::Number(n) => n.as_u64().and_then(|n| u16::try_from(n).ok()),
        _ => None,
    })
}

/// Get a list of strings from a section, with a default fallback.
fn get_string_list_or_default(
    section: &HashMap<String, YamlValue>,
    key: &str,
    default: Vec<String>,
) -> Vec<String> {
    section
        .get(key)
        .and_then(|v| match v {
            YamlValue::Sequence(seq) => {
                let strings: Vec<String> = seq
                    .iter()
                    .filter_map(|item| match item {
                        YamlValue::String(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect();
                if strings.is_empty() {
                    None
                } else {
                    Some(strings)
                }
            }
            _ => None,
        })
        .unwrap_or(default)
}

/// Get an optional string value from a section.
fn get_optional_string(section: &HashMap<String, YamlValue>, key: &str) -> Option<String> {
    section.get(key).and_then(|v| match v {
        YamlValue::String(s) => Some(s.clone()),
        _ => None,
    })
}

/// Parse the tracker kind from the tracker section.
fn parse_tracker_kind(
    tracker: &HashMap<String, YamlValue>,
) -> Result<TrackerKind, ServiceConfigError> {
    let kind_str = get_string_or_default(tracker, "kind", "linear".to_string());
    match kind_str.to_lowercase().as_str() {
        "linear" => Ok(TrackerKind::Linear),
        "github" => Ok(TrackerKind::GitHub),
        "gitlab" => Ok(TrackerKind::GitLab),
        other => Err(ServiceConfigError::UnsupportedTrackerKind(
            other.to_string(),
        )),
    }
}

/// Get the default endpoint for a tracker kind.
fn default_endpoint_for_kind(kind: &TrackerKind) -> String {
    match kind {
        TrackerKind::Linear => "https://api.linear.app/graphql".to_string(),
        TrackerKind::GitHub => "https://api.github.com".to_string(),
        TrackerKind::GitLab => "https://gitlab.com/api/v4".to_string(),
    }
}

/// Resolve the tracker API key, supporting $VAR indirection.
fn resolve_tracker_api_key(
    tracker: &HashMap<String, YamlValue>,
    kind: &TrackerKind,
) -> Result<String, ServiceConfigError> {
    let raw = get_optional_string(tracker, "api_key");

    match raw {
        Some(value) => resolve_env_var(&value),
        None => {
            // Try canonical environment variable based on tracker kind
            let canonical_var = match kind {
                TrackerKind::Linear => "LINEAR_API_KEY",
                TrackerKind::GitHub => "GITHUB_TOKEN",
                TrackerKind::GitLab => "GITLAB_TOKEN",
            };
            match std::env::var(canonical_var) {
                Ok(v) if !v.is_empty() => Ok(v),
                _ => Ok(String::new()), // Will fail validation later
            }
        }
    }
}

/// Resolve $VAR_NAME references in a string value.
///
/// If the value starts with `$`, treat the rest as an environment variable name.
/// If the env var is not set or empty, return an error.
pub fn resolve_env_var(value: &str) -> Result<String, ServiceConfigError> {
    if let Some(var_name) = value.strip_prefix('$') {
        match std::env::var(var_name) {
            Ok(v) if !v.is_empty() => Ok(v),
            _ => Err(ServiceConfigError::MissingEnvVar(var_name.to_string())),
        }
    } else {
        Ok(value.to_string())
    }
}

/// Resolve workspace root path with ~ expansion and relative path resolution.
fn resolve_workspace_root(workspace: &HashMap<String, YamlValue>, workflow_dir: &Path) -> PathBuf {
    let raw = get_optional_string(workspace, "root");

    match raw {
        Some(value) => {
            // Try $VAR resolution first
            let resolved = if value.starts_with('$') {
                resolve_env_var(&value).unwrap_or(value.clone())
            } else {
                value.clone()
            };

            // Expand ~
            let expanded = expand_tilde(&resolved);

            // Resolve relative paths against workflow_dir
            if expanded.is_relative() {
                workflow_dir.join(expanded)
            } else {
                expanded
            }
        }
        None => {
            // Default: <system-temp>/symphony_workspaces
            std::env::temp_dir().join("symphony_workspaces")
        }
    }
}

fn parse_workspace_gc(section: &HashMap<String, YamlValue>) -> WorkspaceGcConfig {
    let defaults = WorkspaceGcConfig::default();
    WorkspaceGcConfig {
        gc_interval_ms: get_u64_or_default(section, "gc_interval_ms", defaults.gc_interval_ms),
        gc_retention_ms: get_u64_or_default(section, "gc_retention_ms", defaults.gc_retention_ms),
        gc_batch_size: get_u64_or_default(section, "gc_batch_size", defaults.gc_batch_size as u64)
            as usize,
        gc_cycle_timeout_ms: get_u64_or_default(
            section,
            "gc_cycle_timeout_ms",
            defaults.gc_cycle_timeout_ms,
        ),
    }
}

/// Expand ~ to the user's home directory.
fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        dirs_or_temp()
    } else if let Some(rest) = path.strip_prefix("~/") {
        dirs_or_temp().join(rest)
    } else {
        PathBuf::from(path)
    }
}

/// Get the home directory or fall back to temp dir.
fn dirs_or_temp() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
}

/// Parse hooks configuration from the hooks section.
fn parse_hooks(section: &HashMap<String, YamlValue>) -> HooksConfig {
    HooksConfig {
        after_create: get_optional_string(section, "after_create"),
        before_run: get_optional_string(section, "before_run"),
        after_run: get_optional_string(section, "after_run"),
        before_remove: get_optional_string(section, "before_remove"),
        timeout_ms: get_u64_or_default(section, "timeout_ms", 60_000),
    }
}

/// Parse codex configuration from the codex section.
fn parse_codex_config(section: &HashMap<String, YamlValue>) -> CodexConfig {
    CodexConfig {
        command: get_string_or_default(section, "command", "codex app-server".to_string()),
        approval_policy: yaml_to_json_value(section.get("approval_policy")),
        thread_sandbox: get_optional_string(section, "thread_sandbox"),
        turn_sandbox_policy: yaml_to_json_value(section.get("turn_sandbox_policy")),
        turn_timeout_ms: get_u64_or_default(section, "turn_timeout_ms", 3_600_000),
        read_timeout_ms: get_u64_or_default(section, "read_timeout_ms", 5_000),
        stall_timeout_ms: get_i64_or_default(section, "stall_timeout_ms", 300_000),
    }
}

/// Convert a YAML value to a serde_json::Value, supporting both strings and maps.
fn yaml_to_json_value(value: Option<&YamlValue>) -> Option<serde_json::Value> {
    match value {
        None | Some(YamlValue::Null) => None,
        Some(YamlValue::String(s)) => Some(serde_json::Value::String(s.clone())),
        Some(YamlValue::Bool(b)) => Some(serde_json::Value::Bool(*b)),
        Some(YamlValue::Number(n)) => {
            if let Some(i) = n.as_i64() {
                Some(serde_json::Value::Number(serde_json::Number::from(i)))
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f).map(serde_json::Value::Number)
            } else {
                None
            }
        }
        Some(YamlValue::Mapping(m)) => {
            let mut map = serde_json::Map::new();
            for (k, v) in m {
                if let YamlValue::String(key) = k {
                    if let Some(json_val) = yaml_to_json_value(Some(v)) {
                        map.insert(key.clone(), json_val);
                    }
                }
            }
            Some(serde_json::Value::Object(map))
        }
        Some(YamlValue::Sequence(seq)) => {
            let arr: Vec<serde_json::Value> = seq
                .iter()
                .filter_map(|v| yaml_to_json_value(Some(v)))
                .collect();
            Some(serde_json::Value::Array(arr))
        }
        _ => None,
    }
}

/// Parse per-state concurrency map from the agent section.
/// State keys are normalized to lowercase. Invalid entries are ignored.
fn parse_state_concurrency_map(agent: &HashMap<String, YamlValue>) -> HashMap<String, usize> {
    let mut result = HashMap::new();

    if let Some(YamlValue::Mapping(mapping)) = agent.get("max_concurrent_agents_by_state") {
        for (k, v) in mapping {
            if let (YamlValue::String(state), YamlValue::Number(n)) = (k, v) {
                if let Some(limit) = n.as_u64() {
                    if limit > 0 {
                        result.insert(state.to_lowercase(), limit as usize);
                    }
                }
            }
        }
    }

    result
}

/// Resolve a value that may contain $VAR references.
/// Alias for `resolve_env_var` for backward compatibility.
pub fn resolve_value(value: &str) -> Result<String, ServiceConfigError> {
    resolve_env_var(value)
}

/// Resolve a path, expanding ~ to home directory and resolving relative paths.
pub fn resolve_path(path_str: &str) -> Result<PathBuf, ServiceConfigError> {
    if path_str.starts_with('~') {
        let home = std::env::var("HOME")
            .map_err(|_| ServiceConfigError::MissingEnvVar("HOME".to_string()))?;
        let rest = path_str.strip_prefix('~').unwrap_or("");
        let rest = rest.strip_prefix('/').unwrap_or(rest);
        Ok(PathBuf::from(home).join(rest))
    } else if path_str.starts_with('/') {
        Ok(PathBuf::from(path_str))
    } else if path_str.starts_with('$') {
        let resolved = resolve_value(path_str)?;
        Ok(PathBuf::from(resolved))
    } else {
        // Relative path — resolve against cwd
        Ok(std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path_str))
    }
}

/// Sanitize a workspace key from an issue identifier.
///
/// Delegates to the canonical implementation in `crate::workspace::sanitize_workspace_key`.
/// This wrapper returns a String (never errors) for backward compatibility:
/// - Empty string returns "_default"
/// - Unsafe identifiers (., ..) return "_default"
/// - All other characters not in [A-Za-z0-9._-] are replaced with '_'
pub fn sanitize_workspace_key(identifier: &str) -> String {
    if identifier.is_empty() {
        return "_default".to_string();
    }

    match crate::workspace::sanitize_workspace_key(identifier) {
        Ok(key) => key,
        Err(_) => "_default".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::workflow_loader::parse_workflow;

    #[test]
    fn test_from_workflow_defaults() {
        let content = "---\ntracker:\n  kind: linear\n  api_key: test-key\n  project_slug: my-project\n---\nPrompt here.\n";
        let workflow = parse_workflow(content).unwrap();
        let config = ServiceConfig::from_workflow(&workflow, Path::new("/tmp")).unwrap();

        assert_eq!(config.tracker_kind, TrackerKind::Linear);
        assert_eq!(config.tracker_endpoint, "https://api.linear.app/graphql");
        assert_eq!(config.tracker_api_key, "test-key");
        assert_eq!(config.tracker_project_slug, "my-project");
        assert_eq!(config.workflow_labels, vec!["Backlog", "Human Review"]);
        assert_eq!(config.poll_interval_ms, 30_000);
        assert_eq!(config.max_concurrent_agents, 10);
        assert_eq!(config.max_turns, 20);
        assert_eq!(config.max_retry_backoff_ms, 300_000);
        assert_eq!(config.codex.command, "codex app-server");
        assert_eq!(config.codex.turn_timeout_ms, 3_600_000);
        assert_eq!(config.codex.read_timeout_ms, 5_000);
        assert_eq!(config.codex.stall_timeout_ms, 300_000);
        assert_eq!(config.hooks.timeout_ms, 60_000);
        assert!(config.hooks.after_create.is_none());
    }

    #[test]
    fn test_from_workflow_custom_values() {
        let content = r#"---
tracker:
  kind: linear
  api_key: my-key
  project_slug: slug
  endpoint: https://custom.endpoint/graphql
  active_states:
    - Todo
    - Working
  terminal_states:
    - Done
  workflow_labels:
    - Backlog
    - Human Review
polling:
  interval_ms: 10000
agent:
  max_concurrent_agents: 5
  max_turns: 10
  max_retry_backoff_ms: 60000
codex:
  command: my-codex-server
  turn_timeout_ms: 1800000
hooks:
  after_create: "git clone repo ."
  before_run: "git pull"
  timeout_ms: 30000
---
Custom prompt.
"#;
        let workflow = parse_workflow(content).unwrap();
        let config = ServiceConfig::from_workflow(&workflow, Path::new("/tmp")).unwrap();

        assert_eq!(config.tracker_endpoint, "https://custom.endpoint/graphql");
        assert_eq!(config.active_states, vec!["Todo", "Working"]);
        assert_eq!(config.terminal_states, vec!["Done"]);
        assert_eq!(config.workflow_labels, vec!["Backlog", "Human Review"]);
        assert_eq!(config.poll_interval_ms, 10_000);
        assert_eq!(config.max_concurrent_agents, 5);
        assert_eq!(config.max_turns, 10);
        assert_eq!(config.max_retry_backoff_ms, 60_000);
        assert_eq!(config.codex.command, "my-codex-server");
        assert_eq!(config.codex.turn_timeout_ms, 1_800_000);
        assert_eq!(
            config.hooks.after_create,
            Some("git clone repo .".to_string())
        );
        assert_eq!(config.hooks.before_run, Some("git pull".to_string()));
        assert_eq!(config.hooks.timeout_ms, 30_000);
    }

    #[test]
    fn test_validate_for_dispatch_missing_api_key() {
        let content = "---\ntracker:\n  kind: linear\n  project_slug: slug\n---\nPrompt.\n";
        let workflow = parse_workflow(content).unwrap();
        let config = ServiceConfig::from_workflow(&workflow, Path::new("/tmp")).unwrap();

        let result = config.validate_for_dispatch();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_for_dispatch_missing_project_slug() {
        let content = "---\ntracker:\n  kind: linear\n  api_key: key\n---\nPrompt.\n";
        let workflow = parse_workflow(content).unwrap();
        let config = ServiceConfig::from_workflow(&workflow, Path::new("/tmp")).unwrap();

        let result = config.validate_for_dispatch();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_for_dispatch_success() {
        let content =
            "---\ntracker:\n  kind: linear\n  api_key: key\n  project_slug: slug\n---\nPrompt.\n";
        let workflow = parse_workflow(content).unwrap();
        let config = ServiceConfig::from_workflow(&workflow, Path::new("/tmp")).unwrap();

        let result = config.validate_for_dispatch();
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_env_var_literal() {
        let result = resolve_env_var("literal-value").unwrap();
        assert_eq!(result, "literal-value");
    }

    #[test]
    fn test_resolve_env_var_reference() {
        std::env::set_var("TEST_SYMPHONY_KEY_SC", "resolved-value");
        let result = resolve_env_var("$TEST_SYMPHONY_KEY_SC").unwrap();
        assert_eq!(result, "resolved-value");
        std::env::remove_var("TEST_SYMPHONY_KEY_SC");
    }

    #[test]
    fn test_resolve_env_var_missing() {
        let result = resolve_env_var("$NONEXISTENT_SYMPHONY_VAR_12345");
        assert!(matches!(result, Err(ServiceConfigError::MissingEnvVar(_))));
    }

    #[test]
    fn test_workspace_root_tilde_expansion() {
        let content = "---\ntracker:\n  kind: linear\n  api_key: k\n  project_slug: s\nworkspace:\n  root: ~/my_workspaces\n---\nP\n";
        let workflow = parse_workflow(content).unwrap();
        let config = ServiceConfig::from_workflow(&workflow, Path::new("/tmp")).unwrap();

        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let expected = PathBuf::from(home).join("my_workspaces");
        assert_eq!(config.workspace_root, expected);
    }

    #[test]
    fn test_workspace_root_relative_path() {
        let content = "---\ntracker:\n  kind: linear\n  api_key: k\n  project_slug: s\nworkspace:\n  root: ./workspaces\n---\nP\n";
        let workflow = parse_workflow(content).unwrap();
        let config = ServiceConfig::from_workflow(&workflow, Path::new("/project/dir")).unwrap();

        assert_eq!(
            config.workspace_root,
            PathBuf::from("/project/dir/./workspaces")
        );
    }

    #[test]
    fn test_unsupported_tracker_kind() {
        let content = "---\ntracker:\n  kind: jira\n  api_key: k\n  project_slug: s\n---\nP\n";
        let workflow = parse_workflow(content).unwrap();
        let result = ServiceConfig::from_workflow(&workflow, Path::new("/tmp"));

        assert!(matches!(
            result,
            Err(ServiceConfigError::UnsupportedTrackerKind(_))
        ));
    }

    #[test]
    fn test_per_state_concurrency_map() {
        let content = r#"---
tracker:
  kind: linear
  api_key: k
  project_slug: s
agent:
  max_concurrent_agents_by_state:
    Todo: 3
    In Progress: 5
---
P
"#;
        let workflow = parse_workflow(content).unwrap();
        let config = ServiceConfig::from_workflow(&workflow, Path::new("/tmp")).unwrap();

        assert_eq!(config.max_concurrent_agents_by_state.get("todo"), Some(&3));
        assert_eq!(
            config.max_concurrent_agents_by_state.get("in progress"),
            Some(&5)
        );
    }
}

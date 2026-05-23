use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

/// Platform-specific configuration (GitHub or GitLab).
#[derive(Debug, Clone, Deserialize)]
pub struct PlatformConfig {
    /// "github" or "gitlab"
    pub kind: String,
    /// Must reference an environment variable with `$` prefix, e.g. "$GITHUB_TOKEN".
    pub api_token: String,
    /// Base URL for the platform API.
    pub base_url: String,
    /// GitHub org/user or GitLab namespace.
    pub owner: String,
    /// Repository name.
    pub repo: String,
    /// GitLab-only: project identifier (numeric ID or namespace/project path).
    pub project_id: Option<String>,
    /// Set to true for self-hosted instances with non-standard hostnames.
    #[serde(default)]
    pub allow_custom_host: bool,
    /// Issue filtering criteria.
    #[serde(default)]
    pub issue_filter: IssueFilter,
    /// Label-based workflow state machine configuration.
    pub workflow: WorkflowConfig,
}

/// Criteria for filtering candidate issues.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct IssueFilter {
    #[serde(default)]
    pub labels: Vec<String>,
    pub assignee: Option<String>,
    pub milestone: Option<String>,
}

/// Workflow state machine configuration using labels.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowConfig {
    /// Mapping from internal state key to label name.
    pub states: HashMap<String, String>,
    /// States that indicate an issue is ready for processing.
    pub active_states: Vec<String>,
    /// States that indicate an issue is complete.
    pub terminal_states: Vec<String>,
}

/// Placeholder for the legacy tracker configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct TrackerConfig {
    pub kind: Option<String>,
}

/// Top-level configuration containing platform and other global settings.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub platform: Option<PlatformConfig>,
    pub tracker: Option<TrackerConfig>,
    #[serde(default = "default_polling_config")]
    pub polling: PollingConfig,
}

fn default_polling_config() -> PollingConfig {
    PollingConfig {
        interval_ms: default_polling_interval_ms(),
    }
}

/// Polling interval configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct PollingConfig {
    #[serde(default = "default_polling_interval_ms")]
    pub interval_ms: u64,
}

fn default_polling_interval_ms() -> u64 {
    30_000
}

impl PollingConfig {
    pub fn interval(&self) -> Duration {
        Duration::from_millis(self.interval_ms)
    }
}

/// HTTP-layer label representation.
#[derive(Debug, Clone, Deserialize)]
pub struct Label {
    pub name: String,
    pub color: Option<String>,
}

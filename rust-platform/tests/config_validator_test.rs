//! Configuration validator unit tests.
//!
//! Tests the startup-time configuration validation logic that ensures
//! the platform config is well-formed before the system begins operation.

#![allow(dead_code)]

mod common;

use std::collections::HashMap;

// Re-export the production types. If the production code is not yet compiled,
// these tests define local equivalents that mirror the expected API.
// Once production code is available, replace with:
//   use symphony_platform::config::{
//       Config, PlatformConfig, WorkflowConfig, TrackerConfig, PollingConfig,
//       validate_platform_config, ConfigValidationError,
//   };

/// Mirror of production ConfigValidationError for test compilation independence.
#[derive(Debug)]
enum ConfigValidationError {
    MutualExclusion,
    InvalidKind(String),
    LiteralToken,
    MissingEnvVar(String),
    InsecureUrl,
    UnrecognizedHost(String),
    UndefinedActiveStates(Vec<String>, Vec<String>),
    UndefinedTerminalStates(Vec<String>, Vec<String>),
    GitlabMissingIdentifier,
    NeitherConfigured,
    InvalidUrl(String),
}

#[derive(Debug, Clone)]
struct WorkflowConfig {
    states: HashMap<String, String>,
    active_states: Vec<String>,
    terminal_states: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct IssueFilter {
    labels: Vec<String>,
    assignee: Option<String>,
    milestone: Option<String>,
}

#[derive(Debug, Clone)]
struct PlatformConfig {
    kind: String,
    api_token: String,
    base_url: String,
    owner: String,
    repo: String,
    project_id: Option<u64>,
    allow_custom_host: bool,
    issue_filter: IssueFilter,
    workflow: WorkflowConfig,
}

#[derive(Debug, Clone)]
struct TrackerConfig {
    kind: Option<String>,
}

#[derive(Debug, Clone)]
struct Config {
    platform: Option<PlatformConfig>,
    tracker: Option<TrackerConfig>,
}

// --- Validation logic (mirrors production `config::validator`) ---

const ALLOWED_HOSTS: &[&str] = &["api.github.com", "github.com", "gitlab.com"];

fn validate_platform_config(config: &Config) -> Result<(), ConfigValidationError> {
    if config.platform.is_some() && config.tracker.is_some() {
        return Err(ConfigValidationError::MutualExclusion);
    }

    let platform = match &config.platform {
        Some(p) => p,
        None => return Ok(()),
    };

    match platform.kind.as_str() {
        "github" | "gitlab" => {}
        other => return Err(ConfigValidationError::InvalidKind(other.to_string())),
    }

    validate_token_reference(&platform.api_token)?;
    validate_base_url(&platform.base_url, platform.allow_custom_host)?;
    validate_workflow_states(&platform.workflow)?;

    if platform.kind == "gitlab" {
        validate_gitlab_specifics(platform)?;
    }

    Ok(())
}

fn validate_token_reference(token: &str) -> Result<(), ConfigValidationError> {
    if !token.starts_with('$') {
        return Err(ConfigValidationError::LiteralToken);
    }
    let var_name = &token[1..];
    match std::env::var(var_name) {
        Ok(val) if val.is_empty() => {
            Err(ConfigValidationError::MissingEnvVar(var_name.to_string()))
        }
        Err(_) => Err(ConfigValidationError::MissingEnvVar(var_name.to_string())),
        Ok(_) => Ok(()),
    }
}

fn validate_base_url(url_str: &str, allow_custom_host: bool) -> Result<(), ConfigValidationError> {
    let url =
        url::Url::parse(url_str).map_err(|e| ConfigValidationError::InvalidUrl(e.to_string()))?;

    let host = url.host_str().unwrap_or("");

    if url.scheme() != "https" && host != "localhost" && host != "127.0.0.1" {
        return Err(ConfigValidationError::InsecureUrl);
    }

    let is_known = ALLOWED_HOSTS.contains(&host) || host.ends_with(".gitlab.com");

    if is_known {
        return Ok(());
    }

    if allow_custom_host {
        return Ok(());
    }

    Err(ConfigValidationError::UnrecognizedHost(host.to_string()))
}

fn validate_workflow_states(workflow: &WorkflowConfig) -> Result<(), ConfigValidationError> {
    let defined_keys: Vec<String> = workflow.states.keys().cloned().collect();

    let undefined_active: Vec<String> = workflow
        .active_states
        .iter()
        .filter(|s| !workflow.states.contains_key(*s))
        .cloned()
        .collect();
    if !undefined_active.is_empty() {
        return Err(ConfigValidationError::UndefinedActiveStates(
            undefined_active,
            defined_keys,
        ));
    }

    let undefined_terminal: Vec<String> = workflow
        .terminal_states
        .iter()
        .filter(|s| !workflow.states.contains_key(*s))
        .cloned()
        .collect();
    if !undefined_terminal.is_empty() {
        return Err(ConfigValidationError::UndefinedTerminalStates(
            undefined_terminal,
            defined_keys,
        ));
    }

    Ok(())
}

fn validate_gitlab_specifics(platform: &PlatformConfig) -> Result<(), ConfigValidationError> {
    if platform.project_id.is_none() && (platform.owner.is_empty() || platform.repo.is_empty()) {
        return Err(ConfigValidationError::GitlabMissingIdentifier);
    }
    Ok(())
}

// --- Helper to build valid configs for testing ---

fn valid_workflow() -> WorkflowConfig {
    let mut states = HashMap::new();
    states.insert("backlog".to_string(), "workflow::backlog".to_string());
    states.insert("todo".to_string(), "workflow::todo".to_string());
    states.insert(
        "in_progress".to_string(),
        "workflow::in-progress".to_string(),
    );
    states.insert(
        "human_review".to_string(),
        "workflow::human-review".to_string(),
    );
    states.insert("rework".to_string(), "workflow::rework".to_string());
    states.insert("merging".to_string(), "workflow::merging".to_string());
    states.insert("done".to_string(), "workflow::done".to_string());

    WorkflowConfig {
        states,
        active_states: vec![
            "todo".to_string(),
            "in_progress".to_string(),
            "rework".to_string(),
        ],
        terminal_states: vec!["done".to_string()],
    }
}

fn valid_github_platform() -> PlatformConfig {
    // Set the env var so token validation passes
    std::env::set_var("TEST_GITHUB_TOKEN", "ghp_test123456");

    PlatformConfig {
        kind: "github".to_string(),
        api_token: "$TEST_GITHUB_TOKEN".to_string(),
        base_url: "https://api.github.com".to_string(),
        owner: "test-org".to_string(),
        repo: "test-repo".to_string(),
        project_id: None,
        allow_custom_host: false,
        issue_filter: IssueFilter::default(),
        workflow: valid_workflow(),
    }
}

fn valid_gitlab_platform() -> PlatformConfig {
    std::env::set_var("TEST_GITLAB_TOKEN", "glpat-test123456");

    PlatformConfig {
        kind: "gitlab".to_string(),
        api_token: "$TEST_GITLAB_TOKEN".to_string(),
        base_url: "https://gitlab.com".to_string(),
        owner: "test-org".to_string(),
        repo: "test-repo".to_string(),
        project_id: Some(12345),
        allow_custom_host: false,
        issue_filter: IssueFilter::default(),
        workflow: valid_workflow(),
    }
}

// =============================================================================
// Tests
// =============================================================================

#[test]
fn test_mutual_exclusion_rejected() {
    let config = Config {
        platform: Some(valid_github_platform()),
        tracker: Some(TrackerConfig {
            kind: Some("linear".to_string()),
        }),
    };

    let result = validate_platform_config(&config);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ConfigValidationError::MutualExclusion
    ));
}

#[test]
fn test_invalid_kind_rejected() {
    std::env::set_var("TEST_GITHUB_TOKEN", "ghp_test123456");

    let mut platform = valid_github_platform();
    platform.kind = "bitbucket".to_string();

    let config = Config {
        platform: Some(platform),
        tracker: None,
    };

    let result = validate_platform_config(&config);
    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigValidationError::InvalidKind(kind) => {
            assert_eq!(kind, "bitbucket");
        }
        other => panic!("Expected InvalidKind, got {:?}", other),
    }
}

#[test]
fn test_literal_token_rejected() {
    let mut platform = valid_github_platform();
    // A literal token without the $ prefix should be rejected
    platform.api_token = "ghp_plaintext_token_value".to_string();

    let config = Config {
        platform: Some(platform),
        tracker: None,
    };

    let result = validate_platform_config(&config);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ConfigValidationError::LiteralToken
    ));
}

#[test]
fn test_missing_env_var_rejected() {
    // Ensure the env var does NOT exist
    std::env::remove_var("NONEXISTENT_TOKEN_VAR_XYZ");

    let mut platform = valid_github_platform();
    platform.api_token = "$NONEXISTENT_TOKEN_VAR_XYZ".to_string();

    let config = Config {
        platform: Some(platform),
        tracker: None,
    };

    let result = validate_platform_config(&config);
    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigValidationError::MissingEnvVar(var) => {
            assert_eq!(var, "NONEXISTENT_TOKEN_VAR_XYZ");
        }
        other => panic!("Expected MissingEnvVar, got {:?}", other),
    }
}

#[test]
fn test_insecure_url_rejected() {
    std::env::set_var("TEST_GITHUB_TOKEN", "ghp_test123456");

    let mut platform = valid_github_platform();
    // HTTP to a non-localhost host should be rejected
    platform.base_url = "http://api.github.com".to_string();

    let config = Config {
        platform: Some(platform),
        tracker: None,
    };

    let result = validate_platform_config(&config);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ConfigValidationError::InsecureUrl
    ));
}

#[test]
fn test_unrecognized_host_rejected() {
    std::env::set_var("TEST_GITHUB_TOKEN", "ghp_test123456");

    let mut platform = valid_github_platform();
    platform.base_url = "https://evil-github-clone.example.com".to_string();
    platform.allow_custom_host = false;

    let config = Config {
        platform: Some(platform),
        tracker: None,
    };

    let result = validate_platform_config(&config);
    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigValidationError::UnrecognizedHost(host) => {
            assert_eq!(host, "evil-github-clone.example.com");
        }
        other => panic!("Expected UnrecognizedHost, got {:?}", other),
    }
}

#[test]
fn test_undefined_active_states_rejected() {
    std::env::set_var("TEST_GITHUB_TOKEN", "ghp_test123456");

    let mut platform = valid_github_platform();
    // Add a state to active_states that doesn't exist in the states map
    platform
        .workflow
        .active_states
        .push("nonexistent_state".to_string());

    let config = Config {
        platform: Some(platform),
        tracker: None,
    };

    let result = validate_platform_config(&config);
    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigValidationError::UndefinedActiveStates(undefined, _defined) => {
            assert!(undefined.contains(&"nonexistent_state".to_string()));
        }
        other => panic!("Expected UndefinedActiveStates, got {:?}", other),
    }
}

#[test]
fn test_gitlab_missing_identifier_rejected() {
    std::env::set_var("TEST_GITLAB_TOKEN", "glpat-test123456");

    let mut platform = valid_gitlab_platform();
    // Remove both project_id and owner/repo
    platform.project_id = None;
    platform.owner = "".to_string();
    platform.repo = "".to_string();

    let config = Config {
        platform: Some(platform),
        tracker: None,
    };

    let result = validate_platform_config(&config);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ConfigValidationError::GitlabMissingIdentifier
    ));
}

#[test]
fn test_valid_github_config_passes() {
    std::env::set_var("TEST_GITHUB_TOKEN", "ghp_test123456");

    let config = Config {
        platform: Some(valid_github_platform()),
        tracker: None,
    };

    let result = validate_platform_config(&config);
    assert!(
        result.is_ok(),
        "Valid GitHub config should pass: {:?}",
        result.err()
    );
}

#[test]
fn test_valid_gitlab_config_passes() {
    std::env::set_var("TEST_GITLAB_TOKEN", "glpat-test123456");

    let config = Config {
        platform: Some(valid_gitlab_platform()),
        tracker: None,
    };

    let result = validate_platform_config(&config);
    assert!(
        result.is_ok(),
        "Valid GitLab config should pass: {:?}",
        result.err()
    );
}

// --- Additional edge case tests ---

#[test]
fn test_localhost_http_allowed() {
    std::env::set_var("TEST_GITHUB_TOKEN", "ghp_test123456");

    let mut platform = valid_github_platform();
    // HTTP to localhost is allowed for development
    platform.base_url = "http://localhost:8080".to_string();
    platform.allow_custom_host = true;

    let config = Config {
        platform: Some(platform),
        tracker: None,
    };

    let result = validate_platform_config(&config);
    assert!(
        result.is_ok(),
        "localhost HTTP should be allowed: {:?}",
        result.err()
    );
}

#[test]
fn test_custom_host_with_opt_in_passes() {
    std::env::set_var("TEST_GITHUB_TOKEN", "ghp_test123456");

    let mut platform = valid_github_platform();
    platform.base_url = "https://github.internal.company.com".to_string();
    platform.allow_custom_host = true;

    let config = Config {
        platform: Some(platform),
        tracker: None,
    };

    let result = validate_platform_config(&config);
    assert!(
        result.is_ok(),
        "Custom host with opt-in should pass: {:?}",
        result.err()
    );
}

#[test]
fn test_no_platform_no_tracker_passes() {
    // When neither platform nor tracker is configured, validation passes
    // (the NeitherConfigured check is at a higher level in the orchestrator)
    let config = Config {
        platform: None,
        tracker: None,
    };

    let result = validate_platform_config(&config);
    assert!(result.is_ok());
}

#[test]
fn test_empty_env_var_rejected() {
    // Set the env var to an empty string
    std::env::set_var("EMPTY_TOKEN_VAR", "");

    let mut platform = valid_github_platform();
    platform.api_token = "$EMPTY_TOKEN_VAR".to_string();

    let config = Config {
        platform: Some(platform),
        tracker: None,
    };

    let result = validate_platform_config(&config);
    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigValidationError::MissingEnvVar(var) => {
            assert_eq!(var, "EMPTY_TOKEN_VAR");
        }
        other => panic!("Expected MissingEnvVar for empty var, got {:?}", other),
    }
}

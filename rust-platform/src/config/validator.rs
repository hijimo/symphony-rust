use thiserror::Error;

use super::platform::{Config, PlatformConfig, WorkflowConfig};

const ALLOWED_HOSTS: &[&str] = &["api.github.com", "github.com", "gitlab.com"];

#[derive(Debug, Error)]
pub enum ConfigValidationError {
    #[error("Cannot configure both 'platform' and 'tracker' simultaneously. Remove one.")]
    MutualExclusion,

    #[error("Unknown platform kind: {0}. Expected 'github', 'gitlab', or 'gitea'.")]
    InvalidKind(String),

    #[error("api_token must reference an environment variable (prefix with $), got literal value")]
    LiteralToken,

    #[error("Environment variable {0} is not set or empty")]
    MissingEnvVar(String),

    #[error("base_url must use HTTPS (exception: localhost for development)")]
    InsecureUrl,

    #[error("base_url '{0}' is not a recognized platform host. For self-hosted instances, add 'allow_custom_host: true'.")]
    UnrecognizedHost(String),

    #[error("base_url must not point to private/reserved IP ranges (RFC 1918, link-local, loopback, cloud metadata)")]
    PrivateNetwork,

    #[error("Gitea owner and repo must contain only alphanumeric, dot, underscore, or hyphen characters")]
    InvalidGiteaSlug,

    #[error("active_states references undefined states: {0:?}. Defined: {1:?}")]
    UndefinedActiveStates(Vec<String>, Vec<String>),

    #[error("terminal_states references undefined states: {0:?}. Defined: {1:?}")]
    UndefinedTerminalStates(Vec<String>, Vec<String>),

    #[error("GitLab requires either project_id or both owner and repo")]
    GitlabMissingIdentifier,

    #[error("Neither 'platform' nor 'tracker' is configured")]
    NeitherConfigured,

    #[error("Invalid URL format: {0}")]
    InvalidUrl(String),
}

/// Validate the platform configuration at startup.
/// Returns Ok(()) if the config is valid or if no platform section is present.
pub fn validate_platform_config(config: &Config) -> Result<(), ConfigValidationError> {
    // platform and tracker cannot both be active
    if config.platform.is_some() && config.tracker.is_some() {
        return Err(ConfigValidationError::MutualExclusion);
    }

    let platform = match &config.platform {
        Some(p) => p,
        None => return Ok(()),
    };

    // Validate kind
    match platform.kind.as_str() {
        "github" | "gitlab" | "gitea" => {}
        other => return Err(ConfigValidationError::InvalidKind(other.to_string())),
    }

    // Validate token reference format
    validate_token_reference(&platform.api_token)?;

    // Validate base_url
    validate_base_url(&platform.base_url, platform.allow_custom_host)?;

    // Validate workflow states mapping completeness
    validate_workflow_states(&platform.workflow)?;

    // GitLab-specific validation
    if platform.kind == "gitlab" {
        validate_gitlab_specifics(platform)?;
    }

    // Gitea-specific validation
    if platform.kind == "gitea" {
        validate_gitea_specifics(platform)?;
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

    // HTTPS required (localhost exception for development)
    if url.scheme() != "https" && host != "localhost" && host != "127.0.0.1" {
        return Err(ConfigValidationError::InsecureUrl);
    }

    // SSRF protection: reject private/reserved IPs even with allow_custom_host
    if is_private_or_reserved(url_str) {
        return Err(ConfigValidationError::PrivateNetwork);
    }

    // Check if it's a known host
    let is_known = ALLOWED_HOSTS.contains(&host) || host.ends_with(".gitlab.com");

    if is_known {
        return Ok(());
    }

    // Self-hosted instances require explicit opt-in
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

fn validate_gitea_specifics(platform: &PlatformConfig) -> Result<(), ConfigValidationError> {
    let valid_pattern = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-')
    };
    if !valid_pattern(&platform.owner) || !valid_pattern(&platform.repo) {
        return Err(ConfigValidationError::InvalidGiteaSlug);
    }
    Ok(())
}

fn is_private_or_reserved(url_str: &str) -> bool {
    let Ok(url) = url::Url::parse(url_str) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };

    if host == "169.254.169.254" || host == "metadata.google.internal" {
        return true;
    }

    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
            }
            std::net::IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
        };
    }

    false
}

use std::path::PathBuf;
use tokio::process::Command;
use tracing::info;

use crate::crypto;
use crate::error::WebPlatformError;
use crate::git_url::Platform;
use crate::handlers::network_proxy::load_effective_proxy_config;
use crate::models::project::Project;
use crate::repository::{SqliteRepository, SystemConfigRepository, UserConfigRepository};
use crate::services::codex_command::resolve_codex_command;
use crate::services::project_repos::{validate_repos_for_spawn, SpawnAccessCheck};
use crate::templates::{self, WorkflowTemplateContext};

pub struct SpawnResult {
    pub pid: u32,
    pub child: tokio::process::Child,
    pub proxy_config_version: String,
}

/// System config key listing extra env var names passed through to workers.
pub const CODEX_ENV_PASSTHROUGH_KEY: &str = "codex.env_passthrough";

/// Parent env vars always passed through to spawned Symphony workers.
///
/// Proxy vars are included so that `inherit_env` mode keeps working; they are
/// then overridden by `EffectiveProxyConfig::apply_to_command`.
pub const BASE_ENV_WHITELIST: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TMPDIR",
    "TERM",
    "CODEX_HOME",
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
];

/// Parse the comma-separated `codex.env_passthrough` value into valid env var names.
pub fn parse_env_passthrough(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|name| {
            !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !name.starts_with(|c: char| c.is_ascii_digit())
        })
        .map(str::to_string)
        .collect()
}

/// Select the parent env vars a worker may inherit: [`BASE_ENV_WHITELIST`] plus
/// `extra` names. Pure function so the whitelist can be unit tested.
pub fn filter_worker_env<I>(parent: I, extra: &[String]) -> Vec<(String, String)>
where
    I: IntoIterator<Item = (String, String)>,
{
    let mut env: Vec<(String, String)> = parent
        .into_iter()
        .filter(|(key, _)| {
            BASE_ENV_WHITELIST.contains(&key.as_str()) || extra.iter().any(|e| e == key)
        })
        .collect();
    env.sort();
    env
}

/// `env_clear` the command, then pass through only whitelisted parent vars.
async fn apply_env_whitelist(cmd: &mut Command, repo: &SqliteRepository) {
    let extra = match repo.list_system_configs().await {
        Ok(configs) => configs
            .into_iter()
            .find(|item| item.key == CODEX_ENV_PASSTHROUGH_KEY)
            .map(|item| parse_env_passthrough(&item.value))
            .unwrap_or_default(),
        Err(error) => {
            tracing::warn!("Failed to load {}: {}", CODEX_ENV_PASSTHROUGH_KEY, error);
            Vec::new()
        }
    };
    cmd.env_clear();
    cmd.envs(filter_worker_env(std::env::vars(), &extra));
}

pub async fn spawn_symphony(
    project: &Project,
    repo: &SqliteRepository,
    encryption_key: &[u8; 32],
    symphony_bin: &str,
    workspace_root: &str,
    service_instance_id: &str,
) -> Result<SpawnResult, WebPlatformError> {
    let owner_id = project.created_by.ok_or_else(|| {
        WebPlatformError::Internal("Project has no owner (created_by is null)".to_string())
    })?;

    let user_config = repo.get_config(owner_id).await?.ok_or_else(|| {
        WebPlatformError::Internal("Project owner has no platform token configured".to_string())
    })?;

    let platform = match project.platform.as_str() {
        "github" => Platform::GitHub,
        "gitea" => Platform::Gitea,
        _ => Platform::GitLab,
    };
    let encrypted_token = match platform {
        Platform::GitLab => user_config.gitlab_token.ok_or_else(|| {
            WebPlatformError::Internal("Project owner has no GitLab token".to_string())
        })?,
        Platform::GitHub => user_config.github_token.ok_or_else(|| {
            WebPlatformError::Internal("Project owner has no GitHub token".to_string())
        })?,
        Platform::Gitea => user_config.gitea_token.ok_or_else(|| {
            WebPlatformError::Internal("Project owner has no Gitea token".to_string())
        })?,
    };

    let token = crypto::decrypt(&encrypted_token, encryption_key)?;

    let proxy_config = load_effective_proxy_config(repo, encryption_key).await?;
    // Multi-repo: re-validate related repositories (refuses to start when invalid).
    let access_check = (platform == Platform::GitLab).then(|| SpawnAccessCheck {
        token: &token,
        proxy_config: &proxy_config,
    });
    let repos = validate_repos_for_spawn(repo, project, access_check).await?;

    let workspace_dir = PathBuf::from(workspace_root).join(project.id.to_string());
    tokio::fs::create_dir_all(&workspace_dir)
        .await
        .map_err(|e| {
            WebPlatformError::Internal(format!("Failed to create workspace dir: {}", e))
        })?;

    let workflow_content = if let Some(ref custom) = project.workflow_content {
        custom.clone()
    } else {
        let project_slug = format!("{}/{}", project.namespace, project.repo_name);
        let platform_host = project
            .platform_host
            .clone()
            .unwrap_or_else(|| match &platform {
                Platform::GitLab => "https://gitlab.com".to_string(),
                Platform::GitHub => "https://github.com".to_string(),
                Platform::Gitea => {
                    tracing::error!("Gitea project missing platform_host");
                    String::new()
                }
            });
        let ctx = WorkflowTemplateContext {
            platform: platform.clone(),
            project_slug,
            platform_host,
            workspace_root: workspace_dir.to_string_lossy().to_string(),
            max_concurrent_agents: project.max_concurrent_agents,
            default_branch: project.default_branch.clone(),
            hooks_after_create: project.hooks_after_create.clone(),
            hooks_before_remove: project.hooks_before_remove.clone(),
            codex_command: resolve_codex_command(repo, project.codex_command.clone()).await,
            codex_approval_policy: project.codex_approval_policy.clone(),
            codex_sandbox: project.codex_sandbox.clone(),
            testing_enabled: project.testing_enabled,
            testing_max_turns: Some(project.testing_max_turns),
            repos,
        };
        templates::render_template(&ctx)
    };

    let workflow_path = workspace_dir.join("WORKFLOW.md");
    tokio::fs::write(&workflow_path, &workflow_content)
        .await
        .map_err(|e| WebPlatformError::Internal(format!("Failed to write WORKFLOW.md: {}", e)))?;

    let mut cmd = Command::new(symphony_bin);
    cmd.arg("WORKFLOW.md");
    cmd.current_dir(&workspace_dir);
    apply_env_whitelist(&mut cmd, repo).await;
    cmd.env("RUST_LOG", "info");
    cmd.env("SYMPHONY_SERVICE_INSTANCE_ID", service_instance_id);

    proxy_config.apply_to_command(&mut cmd);
    let proxy_config_version = proxy_config.version.clone();

    match &platform {
        Platform::GitLab => {
            cmd.env("GITLAB_TOKEN", &token);
            if let Some(ref host) = project.platform_host {
                cmd.env("GITLAB_HOST", host);
            }
        }
        Platform::GitHub => {
            cmd.env("GITHUB_TOKEN", &token);
        }
        Platform::Gitea => {
            cmd.env("GITEA_TOKEN", &token);
            if let Some(ref host) = project.platform_host {
                cmd.env("GITEA_HOST", host);
            }
        }
    }

    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    let log_path = workspace_dir.join("symphony.log");
    let log_file = std::fs::File::create(&log_path)
        .map_err(|e| WebPlatformError::Internal(format!("Failed to create log file: {}", e)))?;
    let log_file_err = log_file.try_clone().map_err(|e| {
        WebPlatformError::Internal(format!("Failed to clone log file handle: {}", e))
    })?;

    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::from(log_file));
    cmd.stderr(std::process::Stdio::from(log_file_err));

    let child = cmd.spawn().map_err(|e| {
        WebPlatformError::Internal(format!(
            "Failed to spawn symphony process (bin={}): {}",
            symphony_bin, e
        ))
    })?;

    let pid = child.id().ok_or_else(|| {
        WebPlatformError::Internal("Spawned process exited immediately".to_string())
    })?;

    info!(
        project_id = project.id,
        pid, "Symphony process spawned successfully"
    );

    Ok(SpawnResult {
        pid,
        child,
        proxy_config_version,
    })
}

pub async fn spawn_test_symphony(
    project: &Project,
    repo: &SqliteRepository,
    encryption_key: &[u8; 32],
    symphony_bin: &str,
    workspace_root: &str,
    service_instance_id: &str,
) -> Result<SpawnResult, WebPlatformError> {
    let owner_id = project.created_by.ok_or_else(|| {
        WebPlatformError::Internal("Project has no owner (created_by is null)".to_string())
    })?;

    let user_config = repo.get_config(owner_id).await?.ok_or_else(|| {
        WebPlatformError::Internal("Project owner has no platform token configured".to_string())
    })?;

    let platform = match project.platform.as_str() {
        "github" => Platform::GitHub,
        "gitea" => Platform::Gitea,
        _ => Platform::GitLab,
    };
    let encrypted_token = match platform {
        Platform::GitLab => user_config.gitlab_token.ok_or_else(|| {
            WebPlatformError::Internal("Project owner has no GitLab token".to_string())
        })?,
        Platform::GitHub => user_config.github_token.ok_or_else(|| {
            WebPlatformError::Internal("Project owner has no GitHub token".to_string())
        })?,
        Platform::Gitea => user_config.gitea_token.ok_or_else(|| {
            WebPlatformError::Internal("Project owner has no Gitea token".to_string())
        })?,
    };

    let token = crypto::decrypt(&encrypted_token, encryption_key)?;

    let proxy_config = load_effective_proxy_config(repo, encryption_key).await?;
    // Test-engineer WORKFLOW_TEST.md is single-repo, but invalid related repos still
    // refuse to start. The GitLab access check already runs for the main service.
    validate_repos_for_spawn(repo, project, None).await?;

    let workspace_dir = PathBuf::from(workspace_root).join(project.id.to_string());
    tokio::fs::create_dir_all(&workspace_dir)
        .await
        .map_err(|e| {
            WebPlatformError::Internal(format!("Failed to create workspace dir: {}", e))
        })?;

    let project_slug = format!("{}/{}", project.namespace, project.repo_name);
    let platform_host = project
        .platform_host
        .clone()
        .unwrap_or_else(|| match &platform {
            Platform::GitLab => "https://gitlab.com".to_string(),
            Platform::GitHub => "https://github.com".to_string(),
            Platform::Gitea => {
                tracing::error!("Gitea project missing platform_host");
                String::new()
            }
        });
    let ctx = WorkflowTemplateContext {
        platform: platform.clone(),
        project_slug,
        platform_host,
        workspace_root: workspace_dir.to_string_lossy().to_string(),
        max_concurrent_agents: project.max_concurrent_agents,
        default_branch: project.default_branch.clone(),
        hooks_after_create: project.hooks_after_create.clone(),
        hooks_before_remove: project.hooks_before_remove.clone(),
        codex_command: resolve_codex_command(repo, project.codex_command.clone()).await,
        codex_approval_policy: project.codex_approval_policy.clone(),
        codex_sandbox: project.codex_sandbox.clone(),
        testing_enabled: project.testing_enabled,
        testing_max_turns: Some(project.testing_max_turns),
        repos: vec![],
    };
    let workflow_content = templates::render_test_template(&ctx);

    let workflow_path = workspace_dir.join("WORKFLOW_TEST.md");
    tokio::fs::write(&workflow_path, &workflow_content)
        .await
        .map_err(|e| {
            WebPlatformError::Internal(format!("Failed to write WORKFLOW_TEST.md: {}", e))
        })?;

    let mut cmd = Command::new(symphony_bin);
    cmd.arg("WORKFLOW_TEST.md");
    cmd.current_dir(&workspace_dir);
    apply_env_whitelist(&mut cmd, repo).await;
    cmd.env("RUST_LOG", "info");
    cmd.env("SYMPHONY_SERVICE_INSTANCE_ID", service_instance_id);

    proxy_config.apply_to_command(&mut cmd);
    let proxy_config_version = proxy_config.version.clone();

    match &platform {
        Platform::GitLab => {
            cmd.env("GITLAB_TOKEN", &token);
            if let Some(ref host) = project.platform_host {
                cmd.env("GITLAB_HOST", host);
            }
        }
        Platform::GitHub => {
            cmd.env("GITHUB_TOKEN", &token);
        }
        Platform::Gitea => {
            cmd.env("GITEA_TOKEN", &token);
            if let Some(ref host) = project.platform_host {
                cmd.env("GITEA_HOST", host);
            }
        }
    }

    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    let log_path = workspace_dir.join("symphony_test.log");
    let log_file = std::fs::File::create(&log_path)
        .map_err(|e| WebPlatformError::Internal(format!("Failed to create log file: {}", e)))?;
    let log_file_err = log_file.try_clone().map_err(|e| {
        WebPlatformError::Internal(format!("Failed to clone log file handle: {}", e))
    })?;

    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::from(log_file));
    cmd.stderr(std::process::Stdio::from(log_file_err));

    let child = cmd.spawn().map_err(|e| {
        WebPlatformError::Internal(format!(
            "Failed to spawn symphony test process (bin={}): {}",
            symphony_bin, e
        ))
    })?;

    let pid = child.id().ok_or_else(|| {
        WebPlatformError::Internal("Spawned test process exited immediately".to_string())
    })?;

    info!(
        project_id = project.id,
        pid, "Symphony test process spawned successfully"
    );

    Ok(SpawnResult {
        pid,
        child,
        proxy_config_version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parent() -> Vec<(String, String)> {
        [
            ("PATH", "/usr/bin"),
            ("HOME", "/home/u"),
            ("LANG", "C.UTF-8"),
            ("HTTPS_PROXY", "http://proxy:8080"),
            ("no_proxy", "localhost"),
            ("CODEX_HOME", "/home/u/.codex"),
            ("GITHUB_TOKEN", "secret"),
            ("OPENAI_API_KEY", "secret"),
            ("SENTRY_AUTH_TOKEN", "secret"),
            ("KIRO_GATEWAY_API_KEY", "kiro"),
            ("JWT_SECRET", "secret"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn whitelist_drops_secrets() {
        let env = filter_worker_env(parent(), &[]);
        let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "CODEX_HOME",
                "HOME",
                "HTTPS_PROXY",
                "LANG",
                "PATH",
                "no_proxy"
            ]
        );
    }

    #[test]
    fn whitelist_passes_configured_extras() {
        let extra = parse_env_passthrough("KIRO_GATEWAY_API_KEY");
        let env = filter_worker_env(parent(), &extra);
        assert!(env.contains(&("KIRO_GATEWAY_API_KEY".to_string(), "kiro".to_string())));
        assert!(!env
            .iter()
            .any(|(k, _)| k == "GITHUB_TOKEN" || k == "OPENAI_API_KEY"));
    }

    #[test]
    fn parse_passthrough_trims_and_rejects_invalid_names() {
        assert_eq!(
            parse_env_passthrough(" KIRO_GATEWAY_API_KEY , ,FOO_1,bad-name,1BAD,A=B"),
            vec!["KIRO_GATEWAY_API_KEY".to_string(), "FOO_1".to_string()]
        );
        assert!(parse_env_passthrough("").is_empty());
    }
}

use std::path::PathBuf;
use tokio::process::Command;
use tracing::info;

use crate::crypto;
use crate::error::WebPlatformError;
use crate::git_url::Platform;
use crate::models::project::Project;
use crate::repository::{SqliteRepository, UserConfigRepository};
use crate::templates::{self, WorkflowTemplateContext};

pub struct SpawnResult {
    pub pid: u32,
    pub child: tokio::process::Child,
}

pub async fn spawn_symphony(
    project: &Project,
    repo: &SqliteRepository,
    encryption_key: &[u8; 32],
    symphony_bin: &str,
    workspace_root: &str,
) -> Result<SpawnResult, WebPlatformError> {
    let owner_id = project.created_by.ok_or_else(|| {
        WebPlatformError::Internal("Project has no owner (created_by is null)".to_string())
    })?;

    let user_config = repo.get_config(owner_id).await?.ok_or_else(|| {
        WebPlatformError::Internal("Project owner has no platform token configured".to_string())
    })?;

    let platform = match project.platform.as_str() {
        "github" => Platform::GitHub,
        _ => Platform::GitLab,
    };
    let encrypted_token = match platform {
        Platform::GitLab => user_config.gitlab_token.ok_or_else(|| {
            WebPlatformError::Internal("Project owner has no GitLab token".to_string())
        })?,
        Platform::GitHub => user_config.github_token.ok_or_else(|| {
            WebPlatformError::Internal("Project owner has no GitHub token".to_string())
        })?,
    };

    let token = crypto::decrypt(&encrypted_token, encryption_key)?;

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
            codex_command: project.codex_command.clone(),
            codex_approval_policy: project.codex_approval_policy.clone(),
            codex_sandbox: project.codex_sandbox.clone(),
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
    cmd.env("RUST_LOG", "info");

    // Inherit proxy environment variables for network access
    for var in [
        "https_proxy",
        "http_proxy",
        "all_proxy",
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "ALL_PROXY",
        "no_proxy",
        "NO_PROXY",
    ] {
        if let Ok(val) = std::env::var(var) {
            cmd.env(var, val);
        }
    }

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

    Ok(SpawnResult { pid, child })
}

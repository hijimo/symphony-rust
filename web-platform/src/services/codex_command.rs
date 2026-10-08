use crate::repository::{SqliteRepository, SystemConfigRepository};

/// Key of the global default Codex launch command in `system_configs`.
pub const CODEX_DEFAULT_COMMAND_KEY: &str = "codex.default_command";

/// Resolve the Codex launch command for a project.
///
/// Priority: the project's own `codex_command`, then the global
/// `codex.default_command` system config. Returns `None` when neither is set so
/// that rust-platform falls back to its built-in default.
pub async fn resolve_codex_command(
    repo: &SqliteRepository,
    project_command: Option<String>,
) -> Option<String> {
    if let Some(command) = project_command {
        if !command.trim().is_empty() {
            return Some(command);
        }
    }

    match repo.list_system_configs().await {
        Ok(configs) => configs
            .into_iter()
            .find(|item| item.key == CODEX_DEFAULT_COMMAND_KEY)
            .map(|item| item.value)
            .filter(|value| !value.trim().is_empty()),
        Err(error) => {
            tracing::warn!("Failed to load {}: {}", CODEX_DEFAULT_COMMAND_KEY, error);
            None
        }
    }
}

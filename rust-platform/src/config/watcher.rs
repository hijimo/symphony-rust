//! Config Watcher — hot-reload support for WORKFLOW.md changes.
//!
//! Uses `notify` crate to watch the WORKFLOW.md file for changes and
//! `arc_swap::ArcSwap` for atomic configuration replacement.
//!
//! Invalid reloads keep the last known good config (SPEC Section 6.2 MUST NOT crash).
//!
//! SPEC reference: Section 6.2

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arc_swap::ArcSwap;
use chrono::{DateTime, Utc};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use super::service_config::{ServiceConfig, ServiceConfigError};
use super::workflow_loader::{load_workflow, WorkflowLoadError};

/// Effective configuration snapshot (config + prompt bundled together).
#[derive(Debug, Clone)]
pub struct EffectiveConfig {
    pub service: ServiceConfig,
    pub prompt_template: String,
    pub loaded_at: DateTime<Utc>,
}

/// Global configuration holder with atomic swap for hot-reload.
///
/// Workers snapshot the config at startup via `load()` and are not affected
/// by subsequent reloads during their lifetime.
pub struct ConfigHolder {
    current: Arc<ArcSwap<EffectiveConfig>>,
    workflow_path: PathBuf,
    /// The file watcher handle — kept alive for the lifetime of ConfigHolder.
    /// When ConfigHolder is dropped, the watcher stops.
    _watcher: Option<RecommendedWatcher>,
}

impl ConfigHolder {
    /// Create a new ConfigHolder with the given initial config, without file watching.
    ///
    /// Use `with_watcher` to enable automatic hot-reload.
    pub fn new(config: EffectiveConfig, workflow_path: PathBuf) -> Self {
        Self {
            current: Arc::new(ArcSwap::from_pointee(config)),
            workflow_path,
            _watcher: None,
        }
    }

    /// Create a ConfigHolder with file watching enabled.
    ///
    /// The watcher monitors the WORKFLOW.md file for changes and automatically
    /// reloads the configuration. Invalid reloads are logged and the last known
    /// good config is preserved.
    pub fn with_watcher(
        config: EffectiveConfig,
        workflow_path: PathBuf,
    ) -> Result<Self, ConfigWatchError> {
        let current = Arc::new(ArcSwap::from_pointee(config));
        let current_clone = current.clone();
        let path_clone = workflow_path.clone();

        // Determine the directory to watch (notify watches directories)
        let watch_dir = workflow_path
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();
        let watch_filename = workflow_path
            .file_name()
            .map(|f| f.to_os_string())
            .unwrap_or_default();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            match res {
                Ok(event) => {
                    // Only react to modify/create events for our specific file
                    let dominated =
                        matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_));
                    if !dominated {
                        return;
                    }

                    // Check if the event is for our workflow file
                    let is_our_file = event
                        .paths
                        .iter()
                        .any(|p| p.file_name().map(|f| f == watch_filename).unwrap_or(false));

                    if !is_our_file {
                        return;
                    }

                    // Attempt reload
                    match try_reload(&path_clone) {
                        Ok(new_config) => {
                            current_clone.store(Arc::new(new_config));
                            tracing::info!("workflow config reloaded successfully");
                        }
                        Err(e) => {
                            tracing::error!(
                                error = %e,
                                "workflow reload failed, keeping last known good config"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "file watcher error");
                }
            }
        })
        .map_err(|e| ConfigWatchError::WatcherInit(e.to_string()))?;

        watcher
            .watch(&watch_dir, RecursiveMode::NonRecursive)
            .map_err(|e| ConfigWatchError::WatcherInit(e.to_string()))?;

        Ok(Self {
            current,
            workflow_path,
            _watcher: Some(watcher),
        })
    }

    /// Load the current effective configuration (atomic snapshot).
    ///
    /// This is cheap (just an atomic pointer load) and safe to call from any thread.
    pub fn load(&self) -> Arc<EffectiveConfig> {
        self.current.load_full()
    }

    /// Store a new effective configuration (atomic swap).
    ///
    /// Used by the watcher callback and for manual/defensive reloads.
    pub fn store(&self, config: EffectiveConfig) {
        self.current.store(Arc::new(config));
    }

    /// Attempt a manual reload of the workflow file.
    ///
    /// This is the defensive re-read mechanism (SPEC Section 6.2 SHOULD):
    /// called periodically by the orchestrator as a fallback in case
    /// notify misses events (NFS, Docker volume mounts, etc.).
    ///
    /// Returns `true` if the config was updated, `false` if unchanged or failed.
    pub fn try_manual_reload(&self) -> bool {
        match try_reload(&self.workflow_path) {
            Ok(new_config) => {
                // Only update if the content actually changed
                let current = self.current.load();
                if current.prompt_template != new_config.prompt_template
                    || current.loaded_at != new_config.loaded_at
                {
                    self.current.store(Arc::new(new_config));
                    tracing::info!("workflow config reloaded via defensive re-read");
                    true
                } else {
                    false
                }
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "defensive workflow re-read failed, keeping last known good config"
                );
                false
            }
        }
    }

    /// Get the workflow file path being watched.
    pub fn workflow_path(&self) -> &Path {
        &self.workflow_path
    }
}

/// Attempt to reload the workflow file and produce a new EffectiveConfig.
fn try_reload(path: &Path) -> Result<EffectiveConfig, ReloadError> {
    let workflow = load_workflow(path).map_err(ReloadError::Load)?;

    let workflow_dir = path.parent().unwrap_or(Path::new("."));
    let service =
        ServiceConfig::from_workflow(&workflow, workflow_dir).map_err(ReloadError::Config)?;

    Ok(EffectiveConfig {
        service,
        prompt_template: workflow.prompt_template,
        loaded_at: Utc::now(),
    })
}

/// Errors from config watching.
#[derive(Debug, thiserror::Error)]
pub enum ConfigWatchError {
    #[error("failed to initialize file watcher: {0}")]
    WatcherInit(String),
}

/// Internal reload error (not exposed publicly — logged and swallowed).
#[derive(Debug, thiserror::Error)]
enum ReloadError {
    #[error("workflow load: {0}")]
    Load(#[from] WorkflowLoadError),

    #[error("config parse: {0}")]
    Config(#[from] ServiceConfigError),
}

impl std::fmt::Display for ConfigHolder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ConfigHolder(path={}, watching={})",
            self.workflow_path.display(),
            self._watcher.is_some()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as IoWrite;
    use tempfile::NamedTempFile;

    fn make_effective_config(prompt: &str) -> EffectiveConfig {
        EffectiveConfig {
            service: ServiceConfig::default(),
            prompt_template: prompt.to_string(),
            loaded_at: Utc::now(),
        }
    }

    #[test]
    fn test_config_holder_load_store() {
        let initial = make_effective_config("initial prompt");
        let holder = ConfigHolder::new(initial, PathBuf::from("/tmp/WORKFLOW.md"));

        let loaded = holder.load();
        assert_eq!(loaded.prompt_template, "initial prompt");

        let updated = make_effective_config("updated prompt");
        holder.store(updated);

        let loaded = holder.load();
        assert_eq!(loaded.prompt_template, "updated prompt");
    }

    #[test]
    fn test_try_reload_valid_file() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            "---\ntracker:\n  kind: linear\n  api_key: k\n  project_slug: s\n---\nHello prompt."
        )
        .unwrap();

        let result = try_reload(file.path());
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.prompt_template, "Hello prompt.");
        assert_eq!(config.service.tracker_api_key, "k");
    }

    #[test]
    fn test_try_reload_missing_file() {
        let result = try_reload(Path::new("/nonexistent/WORKFLOW.md"));
        assert!(result.is_err());
    }

    #[test]
    fn test_try_reload_invalid_yaml() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "---\n- not a map\n---\nPrompt.").unwrap();

        let result = try_reload(file.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_manual_reload_keeps_good_config_on_failure() {
        let initial = make_effective_config("good prompt");
        let holder = ConfigHolder::new(initial, PathBuf::from("/nonexistent/WORKFLOW.md"));

        // Manual reload should fail but not crash
        let updated = holder.try_manual_reload();
        assert!(!updated);

        // Original config preserved
        let loaded = holder.load();
        assert_eq!(loaded.prompt_template, "good prompt");
    }
}

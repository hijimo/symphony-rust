//! Symphony Platform Adapter — entry point.
//!
//! Startup flow (SPEC Section 17.7):
//! 1. Parse CLI args
//! 2. Initialize structured logging
//! 3. Load WORKFLOW.md via WorkflowLoader
//! 4. Construct ServiceConfig from workflow definition
//! 5. Validate configuration for dispatch readiness
//! 6. Construct PromptEngine from prompt template
//! 7. Construct tracker client (LinearClient)
//! 8. Construct WorkspaceManager + startup cleanup
//! 9. Activate ConfigHolder with file watcher (hot reload)
//! 10. Derive DispatchConfig from ServiceConfig
//! 11. Build Orchestrator with all dependencies
//! 12. Optionally start HTTP server (wired to orchestrator channels)
//! 13. Run orchestrator event loop
//! 14. Graceful shutdown on SIGINT/SIGTERM

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use symphony_platform::cli::Cli;
use symphony_platform::config::service_config::{ServiceConfig, TrackerKind};
use symphony_platform::config::watcher::{ConfigHolder, EffectiveConfig};
use symphony_platform::config::workflow_loader::load_workflow;
use symphony_platform::logging::init_logging;
use symphony_platform::models::OrchestratorEvent as ModelOrchestratorEvent;
use symphony_platform::orchestrator::Orchestrator;
use symphony_platform::orchestrator::scheduler::DispatchConfig;
use symphony_platform::platform::github::GithubAdapter;
use symphony_platform::platform::gitlab::GitlabAdapter;
use symphony_platform::prompt::PromptEngine;
use symphony_platform::server;
use symphony_platform::server::api::{OrchestratorEvent, OrchestratorQuery};
use symphony_platform::tracker::gitlab::GitlabTrackerAdapter;
use symphony_platform::tracker::linear::LinearClient;
use symphony_platform::workspace::WorkspaceManager;

/// Default workflow file name when no path is specified.
const DEFAULT_WORKFLOW_FILE: &str = "WORKFLOW.md";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Parse CLI arguments
    let cli = Cli::parse();

    // 2. Initialize structured logging
    init_logging(None);

    tracing::info!("Symphony Platform Adapter starting");

    // 3. Resolve workflow path: CLI arg > env var > default
    let workflow_path = resolve_workflow_path(&cli);
    tracing::info!(path = %workflow_path.display(), "Loading workflow definition");

    // 4. Load WORKFLOW.md via WorkflowLoader
    let workflow = load_workflow(&workflow_path).map_err(|e| {
        tracing::error!(error = %e, "Failed to load workflow file");
        e
    })?;

    // 5. Construct ServiceConfig from workflow definition
    let workflow_dir = workflow_path.parent().unwrap_or(std::path::Path::new("."));
    let service_config = ServiceConfig::from_workflow(&workflow, workflow_dir).map_err(|e| {
        tracing::error!(error = %e, "Failed to parse service configuration");
        e
    })?;

    // 6. Validate configuration for dispatch readiness
    service_config.validate_for_dispatch().map_err(|e| {
        tracing::error!(error = %e, "Configuration validation failed");
        e
    })?;

    tracing::info!("Configuration validated successfully");

    // 7. Construct PromptEngine from prompt template
    let prompt_template = if workflow.prompt_template.is_empty() {
        symphony_platform::prompt::DEFAULT_PROMPT.to_string()
    } else {
        workflow.prompt_template.clone()
    };

    let _prompt_engine = PromptEngine::compile(&prompt_template).map_err(|e| {
        tracing::error!(error = %e, "Failed to compile prompt template");
        e
    })?;

    tracing::info!("Prompt engine compiled");

    // 8. Construct tracker client based on configured kind
    match service_config.tracker_kind {
        TrackerKind::Linear => {
            let _linear_client = LinearClient::new(
                service_config.tracker_endpoint.clone(),
                service_config.tracker_api_key.clone(),
                service_config.tracker_project_slug.clone(),
            )
            .map(|client| client.with_active_states(service_config.active_states.clone()))
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to initialize Linear client");
                e
            })?;
            tracing::info!(
                project_slug = %service_config.tracker_project_slug,
                "Linear tracker client initialized"
            );
        }
        TrackerKind::GitHub => {
            tracing::info!("GitHub tracker mode — tracker client deferred to platform adapter");
        }
        TrackerKind::GitLab => {
            tracing::info!("GitLab tracker mode — tracker client deferred to platform adapter");
        }
    }

    // 9. Construct WorkspaceManager and perform startup cleanup
    let workspace_manager = WorkspaceManager::new(
        service_config.workspace_root.clone(),
        service_config.hooks.clone(),
    );

    // Ensure workspace root directory exists
    tokio::fs::create_dir_all(&service_config.workspace_root)
        .await
        .map_err(|e| {
            tracing::error!(
                path = %service_config.workspace_root.display(),
                error = %e,
                "Failed to create workspace root directory"
            );
            e
        })?;

    // Startup workspace cleanup (SPEC Section 8.6)
    // In a full integration, terminal issue identifiers would come from the tracker.
    // For now, call with empty list to establish the pattern.
    workspace_manager.cleanup_terminal_workspaces(&[]).await;

    tracing::info!(
        root = %service_config.workspace_root.display(),
        "Workspace manager initialized, startup cleanup complete"
    );

    // 10. Activate ConfigHolder with file watcher (hot reload)
    let effective_config = EffectiveConfig {
        service: service_config.clone(),
        prompt_template: prompt_template.clone(),
        loaded_at: chrono::Utc::now(),
    };

    let _config_holder = ConfigHolder::with_watcher(effective_config, workflow_path.clone())
        .unwrap_or_else(|e| {
            tracing::warn!(
                error = %e,
                "Failed to start config file watcher, hot-reload disabled"
            );
            // Fall back to non-watching holder
            ConfigHolder::new(
                EffectiveConfig {
                    service: service_config.clone(),
                    prompt_template: prompt_template.clone(),
                    loaded_at: chrono::Utc::now(),
                },
                workflow_path.clone(),
            )
        });

    tracing::info!("Config watcher activated");

    // 11. Derive DispatchConfig from ServiceConfig
    let dispatch_config = DispatchConfig::from_service_config(&service_config);

    // 12. Set up graceful shutdown
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();

    tokio::spawn(async move {
        shutdown_signal().await;
        tracing::info!("Shutdown signal received");
        cancel_clone.cancel();
    });

    // 13. Build the Orchestrator
    let stall_timeout_ms = service_config.codex.stall_timeout_ms;
    let max_retry_backoff_ms = service_config.max_retry_backoff_ms;
    let mut orchestrator = Orchestrator::new(
        dispatch_config,
        stall_timeout_ms,
        max_retry_backoff_ms,
        cancel.clone(),
    );

    // 14. Wire dependencies into orchestrator
    let workspace_mgr = Arc::new(workspace_manager);
    let prompt_engine_arc = Arc::new(_prompt_engine);
    let config_holder_arc = Arc::new(_config_holder);

    orchestrator.set_workspace_mgr(workspace_mgr.clone());
    orchestrator.set_prompt_engine(prompt_engine_arc);
    orchestrator.set_config_holder(config_holder_arc);

    // Wire tracker based on kind
    match service_config.tracker_kind {
        TrackerKind::Linear => {
            match LinearClient::new(
                service_config.tracker_endpoint.clone(),
                service_config.tracker_api_key.clone(),
                service_config.tracker_project_slug.clone(),
            ) {
                Ok(client) => {
                    orchestrator.set_tracker(Arc::new(
                        client.with_active_states(service_config.active_states.clone()),
                    ));
                    tracing::info!("Linear tracker wired into orchestrator");
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to create Linear client, dispatch disabled");
                }
            }
        }
        TrackerKind::GitLab => {
            let platform_config = build_platform_config(&service_config, "gitlab");

            match GitlabAdapter::new_with_token(platform_config, &service_config.tracker_api_key) {
                Ok(adapter) => {
                    // Ensure workflow labels exist in the project (auto-creates for GitLab)
                    if let Err(e) = adapter.http_client().ensure_workflow_labels().await {
                        tracing::warn!(error = %e, "Failed to verify workflow labels");
                    }

                    let tracker = Arc::new(GitlabTrackerAdapter::new(
                        Arc::new(adapter),
                        service_config.active_states.clone(),
                        service_config.terminal_states.clone(),
                    ));
                    orchestrator.set_tracker(tracker);
                    tracing::info!("GitLab tracker wired into orchestrator");
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to create GitLab adapter, dispatch disabled");
                }
            }
        }
        TrackerKind::GitHub => {
            let platform_config = build_platform_config(&service_config, "github");

            match GithubAdapter::new_with_token(platform_config, &service_config.tracker_api_key) {
                Ok(adapter) => {
                    // Ensure workflow labels exist in the repository
                    if let Err(e) = adapter.http_client().ensure_workflow_labels().await {
                        tracing::warn!(error = %e, "Failed to verify workflow labels");
                    }

                    let tracker = Arc::new(GitlabTrackerAdapter::new(
                        Arc::new(adapter),
                        service_config.active_states.clone(),
                        service_config.terminal_states.clone(),
                    ));
                    orchestrator.set_tracker(tracker);
                    tracing::info!("GitHub tracker wired into orchestrator");
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to create GitHub adapter, dispatch disabled");
                }
            }
        }
    }

    // 15. Optionally start HTTP server (wired to orchestrator channels)
    //     The server port comes from CLI --port override or WORKFLOW.md config.
    let effective_port = cli.port.or(service_config.server_port);

    if let Some(port) = effective_port {
        let (query_tx, mut query_rx) = mpsc::channel::<OrchestratorQuery>(32);
        let (event_tx, mut event_rx) = mpsc::channel::<OrchestratorEvent>(32);

        // Forward HTTP events to the orchestrator's event channel
        let orch_event_tx = orchestrator.event_sender();
        tokio::spawn(async move {
            while let Some(http_event) = event_rx.recv().await {
                match http_event {
                    OrchestratorEvent::ForceRefresh => {
                        let _ = orch_event_tx
                            .send(ModelOrchestratorEvent::ForceRefresh)
                            .await;
                    }
                }
            }
        });

        // Forward HTTP state queries to the orchestrator's serialized event loop.
        let orch_query_tx = orchestrator.event_sender();
        tokio::spawn(async move {
            while let Some(query) = query_rx.recv().await {
                match query {
                    OrchestratorQuery::GetState { reply } => {
                        let _ = orch_query_tx
                            .send(ModelOrchestratorEvent::QueryState { reply })
                            .await;
                    }
                    OrchestratorQuery::GetIssue { identifier, reply } => {
                        let _ = orch_query_tx
                            .send(ModelOrchestratorEvent::QueryIssue { identifier, reply })
                            .await;
                    }
                }
            }
        });

        let cancel_for_server = cancel.clone();
        tokio::spawn(async move {
            let shutdown = async move {
                cancel_for_server.cancelled().await;
            };
            if let Err(e) =
                server::start_http_server_with_shutdown(port, query_tx, event_tx, shutdown).await
            {
                tracing::error!(error = %e, "HTTP server failed");
            }
        });
        tracing::info!(port, "HTTP server extension enabled");
    }

    // 16. Run orchestrator event loop
    tracing::info!("Orchestrator initialized, entering main loop");
    orchestrator.run().await;

    tracing::info!("Symphony Platform Adapter shut down");
    Ok(())
}

/// Resolve the effective workflow path from CLI args, environment, or default.
///
/// Priority:
/// 1. CLI positional argument (WORKFLOW_PATH)
/// 2. SYMPHONY_WORKFLOW environment variable
/// 3. Default: ./WORKFLOW.md in current directory
fn resolve_workflow_path(cli: &Cli) -> PathBuf {
    // CLI arg takes highest priority
    if let Some(ref path) = cli.workflow_path {
        return path.clone();
    }

    // Environment variable
    if let Ok(env_path) = std::env::var("SYMPHONY_WORKFLOW") {
        if !env_path.is_empty() {
            return PathBuf::from(env_path);
        }
    }

    // Default
    PathBuf::from(DEFAULT_WORKFLOW_FILE)
}

fn build_platform_config(
    service_config: &ServiceConfig,
    platform_kind: &str,
) -> symphony_platform::config::platform::PlatformConfig {
    use symphony_platform::config::platform::{IssueFilter, PlatformConfig, WorkflowConfig};

    let base_url = service_config
        .tracker_endpoint
        .trim_end_matches('/')
        .to_string();

    let (owner, repo, project_id) = if platform_kind == "github" {
        let mut parts = service_config.tracker_project_slug.splitn(2, '/');
        let owner = parts.next().unwrap_or_default().to_string();
        let repo = parts.next().unwrap_or_default().to_string();
        (owner, repo, None)
    } else {
        (
            String::new(),
            service_config.tracker_project_slug.clone(),
            Some(service_config.tracker_project_slug.clone()),
        )
    };

    PlatformConfig {
        kind: platform_kind.to_string(),
        api_token: service_config.tracker_api_key.clone(),
        base_url,
        owner,
        repo,
        project_id,
        allow_custom_host: true,
        issue_filter: IssueFilter {
            labels: service_config.active_states.clone(),
            assignee: None,
            milestone: None,
        },
        workflow: WorkflowConfig {
            states: build_workflow_states(service_config),
            active_states: service_config.active_states.clone(),
            terminal_states: service_config.terminal_states.clone(),
        },
    }
}

fn build_workflow_states(
    service_config: &ServiceConfig,
) -> std::collections::HashMap<String, String> {
    let mut states = std::collections::HashMap::new();
    for s in &service_config.active_states {
        states.insert(s.to_lowercase().replace(' ', "_"), s.clone());
    }
    for s in &service_config.terminal_states {
        states.insert(s.to_lowercase().replace(' ', "_"), s.clone());
    }
    for s in &service_config.workflow_labels {
        states.insert(s.to_lowercase().replace(' ', "_"), s.clone());
    }
    states
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_platform::config::service_config::{ServiceConfig, TrackerKind};

    #[test]
    fn platform_config_uses_resolved_tracker_token_value() {
        let mut service_config = ServiceConfig::default();
        service_config.tracker_kind = TrackerKind::GitLab;
        service_config.tracker_endpoint = "https://gitlab.example.com/api/v4".to_string();
        service_config.tracker_api_key = "resolved-token".to_string();
        service_config.tracker_project_slug = "123".to_string();

        let platform_config = build_platform_config(&service_config, "gitlab");

        assert_eq!(platform_config.api_token, "resolved-token");
        assert_eq!(
            platform_config.base_url,
            "https://gitlab.example.com/api/v4"
        );
        assert_eq!(platform_config.project_id, Some("123".to_string()));
    }

    #[test]
    fn platform_config_preserves_github_owner_and_repo() {
        let mut service_config = ServiceConfig::default();
        service_config.tracker_kind = TrackerKind::GitHub;
        service_config.tracker_endpoint = "https://api.github.com".to_string();
        service_config.tracker_api_key = "resolved-token".to_string();
        service_config.tracker_project_slug = "openai/codex".to_string();

        let platform_config = build_platform_config(&service_config, "github");

        assert_eq!(platform_config.owner, "openai");
        assert_eq!(platform_config.repo, "codex");
        assert_eq!(platform_config.project_id, None);
    }

    #[test]
    fn platform_config_includes_non_dispatch_workflow_labels() {
        let mut service_config = ServiceConfig::default();
        service_config.active_states = vec!["Todo".to_string(), "In Progress".to_string()];
        service_config.terminal_states = vec!["Done".to_string()];
        service_config.workflow_labels = vec!["Backlog".to_string(), "Human Review".to_string()];

        let platform_config = build_platform_config(&service_config, "github");

        assert_eq!(
            platform_config.workflow.active_states,
            vec!["Todo", "In Progress"]
        );
        assert_eq!(platform_config.workflow.terminal_states, vec!["Done"]);
        assert!(
            platform_config
                .workflow
                .states
                .values()
                .any(|label| label == "Backlog")
        );
        assert!(
            platform_config
                .workflow
                .states
                .values()
                .any(|label| label == "Human Review")
        );
    }
}

/// Wait for a shutdown signal (SIGINT or SIGTERM on Unix, Ctrl+C on all platforms).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

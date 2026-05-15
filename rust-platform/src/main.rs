//! Symphony Platform Adapter — entry point.
//!
//! Startup flow (SPEC Section 17.7):
//! 1. Parse CLI args
//! 2. Initialize structured logging
//! 3. Load workflow / configuration
//! 4. Validate configuration
//! 5. Build platform adapter
//! 6. Start config watcher (hot reload)
//! 7. Start orchestrator
//! 8. Optionally start HTTP server
//! 9. Graceful shutdown on SIGINT/SIGTERM

use std::sync::Arc;

use clap::Parser;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use symphony_platform::cli::Cli;
use symphony_platform::config::{validate_platform_config, Config};
use symphony_platform::error::PlatformError;
use symphony_platform::logging::init_logging;
use symphony_platform::orchestrator::Orchestrator;
use symphony_platform::platform::cooldown_queue::CooldownQueue;
use symphony_platform::platform::gitlab::GitlabAdapter;
use symphony_platform::platform::Platform;
use symphony_platform::server;
use symphony_platform::server::api::{OrchestratorEvent, OrchestratorQuery};

/// Default config file path (legacy mode, used when no CLI workflow path is given
/// and no WORKFLOW.md exists).
const DEFAULT_CONFIG_PATH: &str = "workflow.yaml";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Parse CLI arguments
    let cli = Cli::parse();

    // 2. Initialize structured logging
    init_logging(None);

    tracing::info!("Symphony Platform Adapter starting");

    // 3. Load configuration
    //    Try WORKFLOW.md path first (new SPEC-compliant mode),
    //    fall back to legacy workflow.yaml for backward compatibility.
    let config = load_config_from_args(&cli)?;

    // 4. Validate configuration
    validate_platform_config(&config).map_err(|e| {
        tracing::error!(error = %e, "Configuration validation failed");
        e
    })?;

    tracing::info!("Configuration validated successfully");

    // 5. Build the platform adapter based on configured kind
    let platform: Arc<dyn Platform> = build_adapter(&config).await?;

    // Validate credentials before starting the main loop
    platform.validate_credentials().await.map_err(|e| {
        tracing::error!(error = %e, "Credential validation failed — check your API token");
        e
    })?;

    tracing::info!("Platform credentials validated");

    // 6. Set up graceful shutdown
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();

    tokio::spawn(async move {
        shutdown_signal().await;
        tracing::info!("Shutdown signal received");
        cancel_clone.cancel();
    });

    // 7. Build cooldown queue
    let cooldown_queue = Arc::new(CooldownQueue::new(config.polling.interval()));
    cooldown_queue.spawn_cleanup_task(cancel.clone());

    // 8. Optionally start HTTP server
    let effective_port = cli.port;
    if let Some(port) = effective_port {
        let (query_tx, _query_rx) = mpsc::channel::<OrchestratorQuery>(32);
        let (event_tx, _event_rx) = mpsc::channel::<OrchestratorEvent>(32);

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

    // 9. Build and run the Orchestrator
    let _config = Arc::new(config);
    let dispatch_config = symphony_platform::orchestrator::scheduler::DispatchConfig::default();
    let mut orchestrator = Orchestrator::new(dispatch_config, 300_000, 300_000, cancel);

    tracing::info!("Orchestrator initialized, entering main loop");
    orchestrator.run().await;

    tracing::info!("Symphony Platform Adapter shut down");
    Ok(())
}

/// Load configuration, trying the CLI workflow path first, then falling back
/// to the legacy workflow.yaml format.
fn load_config_from_args(_cli: &Cli) -> Result<Config, Box<dyn std::error::Error>> {
    // If a workflow path is explicitly provided via CLI, try to use it
    // For now, we still use the legacy YAML config format for the platform adapter.
    // The full SPEC-compliant WORKFLOW.md loading is handled by the config::workflow_loader
    // module and will be integrated when the full orchestrator rewrite is complete.
    let config_path = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("SYMPHONY_CONFIG").ok())
        .unwrap_or_else(|| DEFAULT_CONFIG_PATH.to_string());

    load_config(&config_path)
}

/// Load and parse the YAML configuration file.
fn load_config(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    tracing::info!(path, "Loading configuration");

    let content = std::fs::read_to_string(path).map_err(|e| {
        tracing::error!(path, error = %e, "Failed to read configuration file");
        e
    })?;

    let config: Config = serde_yaml::from_str(&content).map_err(|e| {
        tracing::error!(path, error = %e, "Failed to parse configuration file");
        e
    })?;

    Ok(config)
}

/// Build the appropriate platform adapter based on the configured `kind`.
///
/// Returns an `Arc<dyn Platform>` for use across multiple tokio tasks.
async fn build_adapter(config: &Config) -> Result<Arc<dyn Platform>, PlatformError> {
    let platform_config = config
        .platform
        .as_ref()
        .ok_or_else(|| PlatformError::Unprocessable("No platform configuration found".into()))?;

    match platform_config.kind.as_str() {
        "github" => {
            tracing::info!(
                owner = %platform_config.owner,
                repo = %platform_config.repo,
                "Building GitHub adapter"
            );
            Err(PlatformError::Unprocessable(
                "GitHub adapter not yet available — being implemented by another agent".into(),
            ))
        }
        "gitlab" => {
            tracing::info!(
                project_id = ?platform_config.project_id,
                owner = %platform_config.owner,
                repo = %platform_config.repo,
                "Building GitLab adapter"
            );
            let adapter = GitlabAdapter::new(platform_config.clone())?;
            Ok(Arc::new(adapter))
        }
        other => Err(PlatformError::Unprocessable(format!(
            "Unknown platform kind: '{}'. Expected 'github' or 'gitlab'.",
            other
        ))),
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

pub mod engine;
pub mod metrics;
pub mod rules;

use std::sync::Arc;

use crate::concurrency::ConcurrencyManager;
use crate::process_manager::ProcessManager;
use crate::repository::SqliteRepository;

pub use engine::AlertEngine;
pub use metrics::{
    ConcurrencyMetrics, DefaultMetricCollector, MetricCollector, MetricSnapshot, RunningTask,
    ServiceHealthStatus,
};
pub use rules::{AlertRuleConfig, CooldownManager, DefaultRuleEvaluator, RuleEvaluator};

/// Builder for constructing the alert engine components and spawning the background task.
pub struct AlertEngineBuilder {
    process_manager: ProcessManager,
    concurrency_manager: Arc<ConcurrencyManager>,
    repo: SqliteRepository,
    evaluation_interval_secs: u64,
    runtime_state: Option<Arc<crate::services::runtime_state::RuntimeStateCache>>,
}

/// Handle returned by the builder, containing the components needed by AppState/AlertManager.
pub struct AlertEngineHandle {
    /// Metric collector for external event injection (crash, API failure, task failure).
    pub metric_collector: Arc<DefaultMetricCollector>,
    /// Rule evaluator for hot-reload support.
    pub rule_evaluator: Arc<DefaultRuleEvaluator>,
    /// Cooldown manager for state persistence at shutdown.
    pub cooldown_manager: Arc<CooldownManager>,
    /// Shutdown signal sender for the engine background task.
    pub shutdown_tx: tokio::sync::mpsc::Sender<()>,
}

impl AlertEngineBuilder {
    pub fn new(
        process_manager: ProcessManager,
        concurrency_manager: Arc<ConcurrencyManager>,
        repo: SqliteRepository,
    ) -> Self {
        Self {
            process_manager,
            concurrency_manager,
            repo,
            evaluation_interval_secs: 30,
            runtime_state: None,
        }
    }

    /// Source of running tasks for the task-timeout rule.
    pub fn runtime_state(
        mut self,
        cache: Arc<crate::services::runtime_state::RuntimeStateCache>,
    ) -> Self {
        self.runtime_state = Some(cache);
        self
    }

    pub fn evaluation_interval_secs(mut self, secs: u64) -> Self {
        self.evaluation_interval_secs = secs;
        self
    }

    /// Build and spawn the alert engine. Returns a handle with references to
    /// the internal components for integration with AppState.
    pub async fn build(self) -> AlertEngineHandle {
        // 1. Initialize cooldown manager
        let cooldown_manager = Arc::new(CooldownManager::new());

        // 2. Build metric collector
        let mut collector = DefaultMetricCollector::new(
            self.process_manager.clone(),
            self.concurrency_manager.clone(),
        );
        if let Some(cache) = self.runtime_state.clone() {
            collector = collector.with_runtime_state(cache);
        }
        let metric_collector = Arc::new(collector);

        // 3. Build rule evaluator
        let rule_evaluator = Arc::new(DefaultRuleEvaluator::new(
            cooldown_manager.clone(),
            metric_collector.clone(),
            self.repo.clone(),
        ));

        // Load rules from DB (best-effort at startup)
        if let Err(e) = rule_evaluator.reload_rules().await {
            tracing::warn!("Failed to load alert rules at startup: {}", e);
        }

        // 4. Create shutdown channel
        let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);

        // 5. Build and spawn the engine
        let engine = AlertEngine::new(
            metric_collector.clone(),
            rule_evaluator.clone(),
            self.repo.clone(),
            std::time::Duration::from_secs(self.evaluation_interval_secs),
            shutdown_rx,
        );

        tokio::spawn(async move {
            engine.run().await;
        });

        AlertEngineHandle {
            metric_collector,
            rule_evaluator,
            cooldown_manager,
            shutdown_tx,
        }
    }
}

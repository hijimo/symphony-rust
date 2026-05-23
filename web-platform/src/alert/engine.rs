use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::models::alert::{AlertEvent, InsertAlertHistory};
use crate::repository::{AlertRepository, SqliteRepository};

use super::metrics::{DefaultMetricCollector, MetricCollector};
use super::rules::{DefaultRuleEvaluator, RuleEvaluator};

// ---------------------------------------------------------------------------
// AlertEngine — background tick-based evaluation loop
// ---------------------------------------------------------------------------

/// The AlertEngine drives the collect → evaluate → persist cycle on a
/// configurable interval. It runs as a background tokio task and shuts down
/// gracefully when it receives a signal on the shutdown channel.
pub struct AlertEngine {
    metric_collector: Arc<DefaultMetricCollector>,
    rule_evaluator: Arc<DefaultRuleEvaluator>,
    repo: SqliteRepository,
    evaluation_interval: Duration,
    shutdown_rx: mpsc::Receiver<()>,
}

impl AlertEngine {
    pub fn new(
        metric_collector: Arc<DefaultMetricCollector>,
        rule_evaluator: Arc<DefaultRuleEvaluator>,
        repo: SqliteRepository,
        evaluation_interval: Duration,
        shutdown_rx: mpsc::Receiver<()>,
    ) -> Self {
        Self {
            metric_collector,
            rule_evaluator,
            repo,
            evaluation_interval,
            shutdown_rx,
        }
    }

    /// Run the alert engine main loop. This method blocks until a shutdown
    /// signal is received.
    pub async fn run(mut self) {
        tracing::info!(
            "AlertEngine started (interval: {}s)",
            self.evaluation_interval.as_secs()
        );

        let mut interval = tokio::time::interval(self.evaluation_interval);
        // The first tick completes immediately; skip it to avoid evaluating
        // before the system is fully initialized.
        interval.tick().await;

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.evaluate_cycle().await;
                }
                _ = self.shutdown_rx.recv() => {
                    tracing::info!("AlertEngine received shutdown signal");
                    break;
                }
            }
        }

        tracing::info!("AlertEngine stopped");
    }

    /// Execute a single evaluation cycle: collect metrics, evaluate rules,
    /// and persist any fired alerts.
    async fn evaluate_cycle(&self) {
        // 1. Collect metrics
        let metrics = self.metric_collector.collect().await;

        // 2. Evaluate rules
        let alerts = self.rule_evaluator.evaluate(&metrics).await;

        if alerts.is_empty() {
            return;
        }

        tracing::info!("AlertEngine fired {} alert(s)", alerts.len());

        // 3. Persist each alert to history
        for alert in &alerts {
            if let Err(e) = self.persist_alert(alert).await {
                tracing::error!("Failed to persist alert {}: {}", alert.rule_id, e);
            }
        }

        // 4. Notification dispatch will be handled by the NotificationDispatcher
        //    (implemented by Agent 3). For now, alerts are persisted and logged.
        //    The dispatcher integration point is the alert event list returned here.
    }

    /// Persist a single alert event to the alert_history table.
    async fn persist_alert(&self, alert: &AlertEvent) -> crate::error::Result<()> {
        let context_json = if alert.context.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&alert.context).unwrap_or_default())
        };

        let record = InsertAlertHistory {
            rule_id: alert.rule_id.clone(),
            severity: alert.severity.to_string(),
            project_id: alert.project_id,
            title: alert.title.clone(),
            message: alert.message.clone(),
            context_json,
            fired_at: alert.fired_at.to_rfc3339(),
        };

        self.repo.insert_alert_history(&record).await?;

        Ok(())
    }
}

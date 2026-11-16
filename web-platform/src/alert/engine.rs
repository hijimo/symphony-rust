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
    /// Sends fired alerts to the configured channels (P3 §3 S5).
    dispatcher: Option<Arc<crate::notification::NotificationDispatcher>>,
}

/// Send fired alerts to the channels; returns how many were dispatched.
pub async fn dispatch_alerts(
    dispatcher: &crate::notification::NotificationDispatcher,
    alerts: &[AlertEvent],
) -> usize {
    for a in alerts {
        dispatcher
            .dispatch(&crate::notification::AlertEvent::from(a))
            .await;
    }
    alerts.len()
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
            dispatcher: None,
        }
    }

    pub fn with_dispatcher(
        mut self,
        dispatcher: Option<Arc<crate::notification::NotificationDispatcher>>,
    ) -> Self {
        self.dispatcher = dispatcher;
        self
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

        // 4. Notify the configured channels (severity filters apply).
        if let Some(d) = &self.dispatcher {
            dispatch_alerts(d, &alerts).await;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::alert::Severity;
    use crate::notification::{NotificationChannel, NotificationDispatcher};
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct Rec(Mutex<Vec<String>>);

    #[async_trait::async_trait]
    impl NotificationChannel for Rec {
        fn channel_type(&self) -> &str {
            "test"
        }
        fn channel_id(&self) -> &str {
            "rec"
        }
        async fn send(
            &self,
            a: &crate::notification::AlertEvent,
        ) -> Result<crate::notification::NotificationResult, crate::notification::NotificationError>
        {
            self.0
                .lock()
                .unwrap()
                .push(format!("{}:{}", a.severity, a.title));
            Ok(crate::notification::NotificationResult {
                channel_id: "rec".into(),
                channel_type: "test".into(),
                success: true,
                error_message: None,
                response_time_ms: 0,
                sent_at: chrono::Utc::now(),
            })
        }
        async fn send_test(
            &self,
            _: &str,
            _: &str,
        ) -> Result<crate::notification::NotificationResult, crate::notification::NotificationError>
        {
            unreachable!()
        }
        async fn health_check(&self) -> bool {
            true
        }
    }

    fn alert(sev: Severity, title: &str) -> AlertEvent {
        AlertEvent {
            id: "1".into(),
            rule_id: "service_crash".into(),
            severity: sev,
            project_id: Some(1),
            project_name: Some("p".into()),
            title: title.into(),
            message: "m".into(),
            context: HashMap::new(),
            fired_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn fired_alerts_reach_channels_matching_their_severity() {
        let rec = Arc::new(Rec(Mutex::new(Vec::new())));
        let d = NotificationDispatcher::new();
        d.add_channel(rec.clone(), vec!["critical".into()], true)
            .await;
        let n = dispatch_alerts(
            &d,
            &[
                alert(Severity::Critical, "crash"),
                alert(Severity::Warning, "slow"),
            ],
        )
        .await;
        assert_eq!(n, 2);
        assert_eq!(*rec.0.lock().unwrap(), vec!["critical:crash".to_string()]);
    }
}

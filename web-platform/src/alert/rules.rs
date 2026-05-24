use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use tokio::sync::RwLock;

use crate::error::Result;
use crate::models::alert::{AlertEvent, Severity};
use crate::repository::{AlertRepository, SqliteRepository};

use super::metrics::{
    DefaultMetricCollector, MetricCollector, MetricSnapshot, ServiceHealthStatus,
};

// ---------------------------------------------------------------------------
// AlertRuleConfig — in-memory representation loaded from DB
// ---------------------------------------------------------------------------

/// In-memory representation of an alert rule (loaded from DB).
#[derive(Debug, Clone)]
pub struct AlertRuleConfig {
    pub rule_id: String,
    pub name: String,
    pub description: String,
    pub severity: Severity,
    pub enabled: bool,
    pub threshold: HashMap<String, serde_json::Value>,
    pub cooldown_seconds: i64,
}

// ---------------------------------------------------------------------------
// CooldownManager
// ---------------------------------------------------------------------------

/// Manages cooldown state in memory. Prevents the same rule+scope from firing
/// repeatedly within the configured cooldown window.
pub struct CooldownManager {
    /// (rule_id, scope_key) -> expires_at
    cooldowns: DashMap<(String, String), DateTime<Utc>>,
}

impl CooldownManager {
    pub fn new() -> Self {
        Self {
            cooldowns: DashMap::new(),
        }
    }

    /// Check if a rule is currently in cooldown for the given scope.
    pub fn is_cooling_down(&self, rule_id: &str, scope_key: &str) -> bool {
        let key = (rule_id.to_string(), scope_key.to_string());
        if let Some(expires_at) = self.cooldowns.get(&key) {
            return Utc::now() < *expires_at;
        }
        false
    }

    /// Mark a rule as fired, entering cooldown for the specified duration.
    pub fn mark_fired(&self, rule_id: &str, scope_key: &str, cooldown_seconds: i64) {
        let key = (rule_id.to_string(), scope_key.to_string());
        let expires_at = Utc::now() + chrono::Duration::seconds(cooldown_seconds);
        self.cooldowns.insert(key, expires_at);
    }

    /// Remove expired cooldown entries (call periodically).
    pub fn cleanup_expired(&self) {
        let now = Utc::now();
        self.cooldowns.retain(|_, expires_at| *expires_at > now);
    }

    /// Restore cooldown state from database records (call at startup).
    pub fn restore_from_db(&self, records: Vec<(String, String, DateTime<Utc>)>) {
        let now = Utc::now();
        for (rule_id, scope_key, expires_at) in records {
            if expires_at > now {
                self.cooldowns.insert((rule_id, scope_key), expires_at);
            }
        }
    }

    /// Export current cooldown state for persistence (call at shutdown).
    pub fn export_all(&self) -> Vec<(String, String, DateTime<Utc>)> {
        self.cooldowns
            .iter()
            .map(|entry| {
                let (rule_id, scope_key) = entry.key();
                (rule_id.clone(), scope_key.clone(), *entry.value())
            })
            .collect()
    }
}

impl Default for CooldownManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// RuleEvaluator trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait RuleEvaluator: Send + Sync {
    /// Evaluate all enabled rules against the given metrics snapshot.
    /// Returns a list of alert events that should be fired.
    async fn evaluate(&self, metrics: &MetricSnapshot) -> Vec<AlertEvent>;

    /// Reload rules from the database (called after admin updates rules).
    async fn reload_rules(&self) -> Result<()>;
}

// ---------------------------------------------------------------------------
// DefaultRuleEvaluator
// ---------------------------------------------------------------------------

/// Default rule evaluator that implements all 6 alert rules.
pub struct DefaultRuleEvaluator {
    rules: RwLock<Vec<AlertRuleConfig>>,
    cooldown_manager: Arc<CooldownManager>,
    metric_collector: Arc<DefaultMetricCollector>,
    repo: SqliteRepository,
}

impl DefaultRuleEvaluator {
    pub fn new(
        cooldown_manager: Arc<CooldownManager>,
        metric_collector: Arc<DefaultMetricCollector>,
        repo: SqliteRepository,
    ) -> Self {
        Self {
            rules: RwLock::new(Vec::new()),
            cooldown_manager,
            metric_collector,
            repo,
        }
    }

    /// Get a reference to the cooldown manager.
    pub fn cooldown_manager(&self) -> &Arc<CooldownManager> {
        &self.cooldown_manager
    }

    // --- Individual rule evaluators ---

    /// Rule: task_timeout
    /// Fires when any running task exceeds the configured timeout threshold.
    fn evaluate_task_timeout(
        &self,
        rule: &AlertRuleConfig,
        metrics: &MetricSnapshot,
    ) -> Vec<AlertEvent> {
        let timeout_minutes = rule
            .threshold
            .get("timeout_minutes")
            .and_then(|v| v.as_u64())
            .unwrap_or(30);

        let mut alerts = Vec::new();

        for (project_id, tasks) in &metrics.running_tasks {
            for task in tasks {
                let elapsed_minutes = task.elapsed_seconds / 60;
                if elapsed_minutes >= timeout_minutes as i64 {
                    let scope_key = format!("project:{}:issue:{}", project_id, task.issue_iid);

                    if !self
                        .cooldown_manager
                        .is_cooling_down(&rule.rule_id, &scope_key)
                    {
                        alerts.push(AlertEvent {
                            id: uuid::Uuid::new_v4().to_string(),
                            rule_id: rule.rule_id.clone(),
                            severity: rule.severity,
                            project_id: Some(*project_id),
                            project_name: None,
                            title: "任务超时告警".to_string(),
                            message: format!(
                                "Issue #{} ({}) 运行时间已超过 {} 分钟（当前 {} 分钟）",
                                task.issue_iid, task.issue_title, timeout_minutes, elapsed_minutes
                            ),
                            context: HashMap::from([
                                ("issue_iid".to_string(), task.issue_iid.to_string()),
                                ("issue_title".to_string(), task.issue_title.clone()),
                                ("duration_minutes".to_string(), elapsed_minutes.to_string()),
                                ("threshold_minutes".to_string(), timeout_minutes.to_string()),
                                ("agent_id".to_string(), task.agent_id.clone()),
                            ]),
                            fired_at: Utc::now(),
                        });

                        self.cooldown_manager.mark_fired(
                            &rule.rule_id,
                            &scope_key,
                            rule.cooldown_seconds,
                        );
                    }
                }
            }
        }

        alerts
    }

    /// Rule: service_crash
    /// Fires when ProcessManager detects an unexpected process exit.
    fn evaluate_service_crash(
        &self,
        rule: &AlertRuleConfig,
        metrics: &MetricSnapshot,
    ) -> Vec<AlertEvent> {
        let mut alerts = Vec::new();

        for (project_id, health) in &metrics.service_health {
            if let ServiceHealthStatus::Crashed {
                exit_code,
                crashed_at,
            } = health
            {
                let scope_key = format!("project:{}", project_id);

                if !self
                    .cooldown_manager
                    .is_cooling_down(&rule.rule_id, &scope_key)
                {
                    alerts.push(AlertEvent {
                        id: uuid::Uuid::new_v4().to_string(),
                        rule_id: rule.rule_id.clone(),
                        severity: rule.severity,
                        project_id: Some(*project_id),
                        project_name: None,
                        title: "服务异常退出".to_string(),
                        message: format!("Symphony 实例进程意外退出 (exit code: {})", exit_code),
                        context: HashMap::from([
                            ("exit_code".to_string(), exit_code.to_string()),
                            ("crashed_at".to_string(), crashed_at.to_rfc3339()),
                        ]),
                        fired_at: Utc::now(),
                    });

                    self.cooldown_manager.mark_fired(
                        &rule.rule_id,
                        &scope_key,
                        rule.cooldown_seconds,
                    );
                }
            }
        }

        alerts
    }

    /// Rule: concurrency_saturation
    /// Fires when global concurrency is at max capacity for longer than threshold.
    fn evaluate_concurrency_saturation(
        &self,
        rule: &AlertRuleConfig,
        metrics: &MetricSnapshot,
    ) -> Vec<AlertEvent> {
        let saturation_minutes = rule
            .threshold
            .get("saturation_minutes")
            .and_then(|v| v.as_u64())
            .unwrap_or(10);

        let mut alerts = Vec::new();

        if metrics.concurrency.global_active >= metrics.concurrency.global_max
            && metrics.concurrency.global_max > 0
        {
            if let Some(saturated_since) = metrics.concurrency.saturated_since {
                let saturated_minutes = (Utc::now() - saturated_since).num_minutes();
                if saturated_minutes >= saturation_minutes as i64 {
                    let scope_key = "global".to_string();

                    if !self
                        .cooldown_manager
                        .is_cooling_down(&rule.rule_id, &scope_key)
                    {
                        alerts.push(AlertEvent {
                            id: uuid::Uuid::new_v4().to_string(),
                            rule_id: rule.rule_id.clone(),
                            severity: rule.severity,
                            project_id: None,
                            project_name: None,
                            title: "并行饱和告警".to_string(),
                            message: format!(
                                "全局并行数已达上限 ({}/{}) 持续 {} 分钟",
                                metrics.concurrency.global_active,
                                metrics.concurrency.global_max,
                                saturated_minutes
                            ),
                            context: HashMap::from([
                                (
                                    "global_active".to_string(),
                                    metrics.concurrency.global_active.to_string(),
                                ),
                                (
                                    "global_max".to_string(),
                                    metrics.concurrency.global_max.to_string(),
                                ),
                                (
                                    "saturated_minutes".to_string(),
                                    saturated_minutes.to_string(),
                                ),
                                (
                                    "threshold_minutes".to_string(),
                                    saturation_minutes.to_string(),
                                ),
                            ]),
                            fired_at: Utc::now(),
                        });

                        self.cooldown_manager.mark_fired(
                            &rule.rule_id,
                            &scope_key,
                            rule.cooldown_seconds,
                        );
                    }
                }
            }
        }

        alerts
    }

    /// Rule: consecutive_failures
    /// Fires when a project has N consecutive task failures.
    fn evaluate_consecutive_failures(
        &self,
        rule: &AlertRuleConfig,
        _metrics: &MetricSnapshot,
    ) -> Vec<AlertEvent> {
        let failure_count_threshold = rule
            .threshold
            .get("failure_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(3);

        let mut alerts = Vec::new();

        // Check all projects that have process state
        for entry in self.metric_collector.process_manager.processes.iter() {
            let project_id = *entry.key();
            let failures = self.metric_collector.get_consecutive_failures(project_id);

            if failures >= failure_count_threshold {
                let scope_key = format!("project:{}", project_id);

                if !self
                    .cooldown_manager
                    .is_cooling_down(&rule.rule_id, &scope_key)
                {
                    alerts.push(AlertEvent {
                        id: uuid::Uuid::new_v4().to_string(),
                        rule_id: rule.rule_id.clone(),
                        severity: rule.severity,
                        project_id: Some(project_id),
                        project_name: None,
                        title: "连续失败告警".to_string(),
                        message: format!(
                            "项目连续 {} 个任务失败（阈值 {}）",
                            failures, failure_count_threshold
                        ),
                        context: HashMap::from([
                            ("consecutive_failures".to_string(), failures.to_string()),
                            ("threshold".to_string(), failure_count_threshold.to_string()),
                        ]),
                        fired_at: Utc::now(),
                    });

                    self.cooldown_manager.mark_fired(
                        &rule.rule_id,
                        &scope_key,
                        rule.cooldown_seconds,
                    );
                }
            }
        }

        alerts
    }

    /// Rule: api_unreachable
    /// Fires when a platform API has consecutive failures exceeding threshold.
    fn evaluate_api_unreachable(
        &self,
        rule: &AlertRuleConfig,
        metrics: &MetricSnapshot,
    ) -> Vec<AlertEvent> {
        let failure_count_threshold = rule
            .threshold
            .get("failure_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(5);

        let mut alerts = Vec::new();

        for (platform, consecutive_failures) in &metrics.api_health {
            if *consecutive_failures >= failure_count_threshold {
                let scope_key = format!("platform:{}", platform);

                if !self
                    .cooldown_manager
                    .is_cooling_down(&rule.rule_id, &scope_key)
                {
                    alerts.push(AlertEvent {
                        id: uuid::Uuid::new_v4().to_string(),
                        rule_id: rule.rule_id.clone(),
                        severity: rule.severity,
                        project_id: None,
                        project_name: None,
                        title: "API 不可达告警".to_string(),
                        message: format!(
                            "{} API 连续 {} 次请求失败（阈值 {}）",
                            platform, consecutive_failures, failure_count_threshold
                        ),
                        context: HashMap::from([
                            ("platform".to_string(), platform.clone()),
                            (
                                "consecutive_failures".to_string(),
                                consecutive_failures.to_string(),
                            ),
                            ("threshold".to_string(), failure_count_threshold.to_string()),
                        ]),
                        fired_at: Utc::now(),
                    });

                    self.cooldown_manager.mark_fired(
                        &rule.rule_id,
                        &scope_key,
                        rule.cooldown_seconds,
                    );
                }
            }
        }

        alerts
    }
}

#[async_trait]
impl RuleEvaluator for DefaultRuleEvaluator {
    async fn evaluate(&self, metrics: &MetricSnapshot) -> Vec<AlertEvent> {
        let rules = self.rules.read().await;
        let mut all_alerts = Vec::new();

        for rule in rules.iter() {
            if !rule.enabled {
                continue;
            }

            let alerts = match rule.rule_id.as_str() {
                "task_timeout" => self.evaluate_task_timeout(rule, metrics),
                "task_failure" => {
                    // task_failure is event-driven, not tick-based.
                    // It fires via record_task_failure -> consecutive check.
                    // No periodic evaluation needed.
                    Vec::new()
                }
                "service_crash" => self.evaluate_service_crash(rule, metrics),
                "concurrency_saturation" => self.evaluate_concurrency_saturation(rule, metrics),
                "consecutive_failures" => self.evaluate_consecutive_failures(rule, metrics),
                "api_unreachable" => self.evaluate_api_unreachable(rule, metrics),
                _ => {
                    tracing::warn!("Unknown rule_id: {}", rule.rule_id);
                    Vec::new()
                }
            };

            all_alerts.extend(alerts);
        }

        // Periodically clean up expired cooldowns
        self.cooldown_manager.cleanup_expired();

        all_alerts
    }

    async fn reload_rules(&self) -> Result<()> {
        let db_rules = self.repo.get_all_alert_rules().await?;
        let mut rules = self.rules.write().await;
        rules.clear();

        for row in db_rules {
            rules.push(AlertRuleConfig {
                rule_id: row.rule_id,
                name: row.name,
                description: row.description,
                severity: Severity::parse_or_info(&row.severity),
                enabled: row.enabled,
                threshold: row.threshold,
                cooldown_seconds: row.cooldown_seconds,
            });
        }

        tracing::info!("Reloaded {} alert rules", rules.len());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alert::metrics::{
        ConcurrencyMetrics, MetricSnapshot, RunningTask, ServiceHealthStatus,
    };
    use crate::models::alert::Severity;
    use chrono::{Duration, Utc};
    use std::collections::HashMap;

    /// Helper: build a minimal AlertRuleConfig for testing.
    fn make_rule(rule_id: &str, threshold: HashMap<String, serde_json::Value>) -> AlertRuleConfig {
        AlertRuleConfig {
            rule_id: rule_id.to_string(),
            name: format!("Test {}", rule_id),
            description: "test rule".to_string(),
            severity: Severity::Warning,
            enabled: true,
            threshold,
            cooldown_seconds: 300,
        }
    }

    /// Helper: build an empty MetricSnapshot.
    fn empty_snapshot() -> MetricSnapshot {
        MetricSnapshot {
            running_tasks: HashMap::new(),
            service_health: HashMap::new(),
            concurrency: ConcurrencyMetrics {
                global_max: 10,
                global_active: 0,
                saturated_since: None,
            },
            api_health: HashMap::new(),
            collected_at: Utc::now(),
        }
    }

    /// Helper: build a DefaultRuleEvaluator for unit tests (no DB needed for
    /// individual rule evaluation methods).
    fn make_evaluator() -> (DefaultRuleEvaluator, Arc<CooldownManager>) {
        use crate::concurrency::ConcurrencyManager;
        use crate::process_manager::ProcessManager;
        use crate::repository::SqliteRepository;

        // We need a minimal SqliteRepository. Use an in-memory DB.
        let pool = crate::db::init_pool(":memory:");
        let repo = SqliteRepository::new(pool);

        let pm = ProcessManager::new();
        let cm = Arc::new(ConcurrencyManager::new(10));
        let cooldown = Arc::new(CooldownManager::new());
        let mc = Arc::new(super::super::metrics::DefaultMetricCollector::new(pm, cm));

        let evaluator = DefaultRuleEvaluator::new(cooldown.clone(), mc, repo);
        (evaluator, cooldown)
    }

    // =========================================================================
    // task_timeout tests
    // =========================================================================

    #[test]
    fn test_task_timeout_fires_when_exceeded() {
        let (evaluator, _cooldown) = make_evaluator();
        let rule = make_rule(
            "task_timeout",
            HashMap::from([("timeout_minutes".to_string(), serde_json::json!(30))]),
        );

        let mut snapshot = empty_snapshot();
        snapshot.running_tasks.insert(
            1,
            vec![RunningTask {
                agent_id: "agent-1".to_string(),
                issue_iid: 42,
                issue_title: "Fix bug".to_string(),
                project_id: 1,
                started_at: Utc::now() - Duration::minutes(45),
                elapsed_seconds: 45 * 60,
            }],
        );

        let alerts = evaluator.evaluate_task_timeout(&rule, &snapshot);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].rule_id, "task_timeout");
        assert!(alerts[0].message.contains("42"));
    }

    #[test]
    fn test_task_timeout_no_fire_within_threshold() {
        let (evaluator, _cooldown) = make_evaluator();
        let rule = make_rule(
            "task_timeout",
            HashMap::from([("timeout_minutes".to_string(), serde_json::json!(30))]),
        );

        let mut snapshot = empty_snapshot();
        snapshot.running_tasks.insert(
            1,
            vec![RunningTask {
                agent_id: "agent-1".to_string(),
                issue_iid: 10,
                issue_title: "Normal task".to_string(),
                project_id: 1,
                started_at: Utc::now() - Duration::minutes(15),
                elapsed_seconds: 15 * 60,
            }],
        );

        let alerts = evaluator.evaluate_task_timeout(&rule, &snapshot);
        assert!(alerts.is_empty());
    }

    // =========================================================================
    // service_crash tests
    // =========================================================================

    #[test]
    fn test_service_crash_fires_on_unexpected_exit() {
        let (evaluator, _cooldown) = make_evaluator();
        let rule = make_rule("service_crash", HashMap::new());

        let mut snapshot = empty_snapshot();
        snapshot.service_health.insert(
            1,
            ServiceHealthStatus::Crashed {
                exit_code: 137,
                crashed_at: Utc::now(),
            },
        );

        let alerts = evaluator.evaluate_service_crash(&rule, &snapshot);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].rule_id, "service_crash");
        assert!(alerts[0].message.contains("137"));
    }

    // =========================================================================
    // concurrency_saturation tests
    // =========================================================================

    #[test]
    fn test_concurrency_saturation_fires_after_duration() {
        let (evaluator, _cooldown) = make_evaluator();
        let rule = make_rule(
            "concurrency_saturation",
            HashMap::from([("saturation_minutes".to_string(), serde_json::json!(10))]),
        );

        let mut snapshot = empty_snapshot();
        snapshot.concurrency = ConcurrencyMetrics {
            global_max: 5,
            global_active: 5,
            saturated_since: Some(Utc::now() - Duration::minutes(15)),
        };

        let alerts = evaluator.evaluate_concurrency_saturation(&rule, &snapshot);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].rule_id, "concurrency_saturation");
    }

    #[test]
    fn test_concurrency_saturation_no_fire_below_duration() {
        let (evaluator, _cooldown) = make_evaluator();
        let rule = make_rule(
            "concurrency_saturation",
            HashMap::from([("saturation_minutes".to_string(), serde_json::json!(10))]),
        );

        let mut snapshot = empty_snapshot();
        snapshot.concurrency = ConcurrencyMetrics {
            global_max: 5,
            global_active: 5,
            saturated_since: Some(Utc::now() - Duration::minutes(3)),
        };

        let alerts = evaluator.evaluate_concurrency_saturation(&rule, &snapshot);
        assert!(alerts.is_empty());
    }

    // =========================================================================
    // consecutive_failures tests
    // =========================================================================

    #[test]
    fn test_consecutive_failures_fires_at_threshold() {
        let (evaluator, _cooldown) = make_evaluator();
        let rule = make_rule(
            "consecutive_failures",
            HashMap::from([("failure_count".to_string(), serde_json::json!(3))]),
        );

        // Simulate a project in the process manager and record failures
        let project_id = 100;
        evaluator.metric_collector.process_manager.processes.insert(
            project_id,
            crate::process_manager::ProcessState {
                pid: 1234,
                started_at: Utc::now(),
                status: crate::models::ServiceStatus::Running,
                restart_count: 0,
            },
        );

        // Record 3 failures
        evaluator
            .metric_collector
            .record_task_failure(project_id, "agent-1", 1);
        evaluator
            .metric_collector
            .record_task_failure(project_id, "agent-1", 2);
        evaluator
            .metric_collector
            .record_task_failure(project_id, "agent-1", 3);

        let snapshot = empty_snapshot();
        let alerts = evaluator.evaluate_consecutive_failures(&rule, &snapshot);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].rule_id, "consecutive_failures");
    }

    #[test]
    fn test_consecutive_failures_no_fire_below_threshold() {
        let (evaluator, _cooldown) = make_evaluator();
        let rule = make_rule(
            "consecutive_failures",
            HashMap::from([("failure_count".to_string(), serde_json::json!(3))]),
        );

        let project_id = 200;
        evaluator.metric_collector.process_manager.processes.insert(
            project_id,
            crate::process_manager::ProcessState {
                pid: 5678,
                started_at: Utc::now(),
                status: crate::models::ServiceStatus::Running,
                restart_count: 0,
            },
        );

        // Only 2 failures — below threshold of 3
        evaluator
            .metric_collector
            .record_task_failure(project_id, "agent-1", 1);
        evaluator
            .metric_collector
            .record_task_failure(project_id, "agent-1", 2);

        let snapshot = empty_snapshot();
        let alerts = evaluator.evaluate_consecutive_failures(&rule, &snapshot);
        assert!(alerts.is_empty());
    }

    // =========================================================================
    // api_unreachable tests
    // =========================================================================

    #[test]
    fn test_api_unreachable_fires_at_threshold() {
        let (evaluator, _cooldown) = make_evaluator();
        let rule = make_rule(
            "api_unreachable",
            HashMap::from([("failure_count".to_string(), serde_json::json!(5))]),
        );

        let mut snapshot = empty_snapshot();
        snapshot.api_health.insert("gitlab".to_string(), 5);

        let alerts = evaluator.evaluate_api_unreachable(&rule, &snapshot);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].rule_id, "api_unreachable");
        assert!(alerts[0].message.contains("gitlab"));
    }

    // =========================================================================
    // cooldown tests
    // =========================================================================

    #[test]
    fn test_cooldown_prevents_duplicate_fire() {
        let (evaluator, cooldown) = make_evaluator();
        let rule = make_rule("service_crash", HashMap::new());

        let mut snapshot = empty_snapshot();
        snapshot.service_health.insert(
            1,
            ServiceHealthStatus::Crashed {
                exit_code: 1,
                crashed_at: Utc::now(),
            },
        );

        // First evaluation should fire
        let alerts = evaluator.evaluate_service_crash(&rule, &snapshot);
        assert_eq!(alerts.len(), 1);

        // Second evaluation should be suppressed by cooldown
        let alerts = evaluator.evaluate_service_crash(&rule, &snapshot);
        assert!(alerts.is_empty());

        // Verify cooldown is active
        assert!(cooldown.is_cooling_down("service_crash", "project:1"));
    }

    #[test]
    fn test_cooldown_expires_allows_refire() {
        let cooldown = CooldownManager::new();

        // Mark fired with a very short cooldown (already expired)
        let key = ("test_rule".to_string(), "scope:1".to_string());
        let expired = Utc::now() - Duration::seconds(10);
        cooldown.cooldowns.insert(key, expired);

        // Should not be cooling down since it's expired
        assert!(!cooldown.is_cooling_down("test_rule", "scope:1"));
    }
}
